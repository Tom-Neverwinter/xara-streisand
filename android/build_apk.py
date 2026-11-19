#!/usr/bin/env python3
"""Build the xara Android app (an APK) from this repo with the Android SDK's own tools: no Gradle.

    python android/build_apk.py [--abis arm64-v8a,armeabi-v7a,x86_64] [--pat] [--out xara.apk]

Needs ANDROID_HOME (SDK with build-tools and a platform), the NDK (ANDROID_NDK_HOME, ANDROID_NDK_LATEST_HOME or
the newest under $ANDROID_HOME/ndk), a JDK 17+, rustup targets for each ABI, and Go for --pat. The modem
(xara, xara-monitor) and Pat go in as lib*.so executables; the app runs them under a foreground service.
Signed with a debug key unless XARA_KEYSTORE / XARA_KEYSTORE_PASS / XARA_KEY_ALIAS name a release key.
"""
import argparse
import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
WIN = os.name == "nt"
API = 26  # AAudio's minimum
TARGET_SDK = 35
PAT_VERSION = "v1.0.1"
ABIS = {
    # Android ABI: (Rust target, NDK clang prefix, GOARCH, GOARM)
    "arm64-v8a": ("aarch64-linux-android", "aarch64-linux-android", "arm64", ""),
    "armeabi-v7a": ("armv7-linux-androideabi", "armv7a-linux-androideabi", "arm", "7"),
    "x86_64": ("x86_64-linux-android", "x86_64-linux-android", "amd64", ""),
}


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, **kw)


def files(root, ext):
    """Every *ext file under root (glob would read the [ ] a path can hold as a pattern)."""
    return [os.path.join(d, f) for d, _, fs in os.walk(root) for f in fs if f.endswith(ext)]


def rmtree(p):
    """Remove a tree, read-only files included (Go's module cache marks its files read-only)."""
    def again(func, path, _):
        os.chmod(path, 0o700)
        func(path)
    if os.path.exists(p):
        shutil.rmtree(p, onexc=again) if sys.version_info >= (3, 12) else shutil.rmtree(p, onerror=again)


def newest(paths):
    def key(p):
        return [int(x) if x.isdigit() else x for x in re.split(r"[.\-]", os.path.basename(p))]
    paths = [p for p in paths if os.path.isdir(p)]
    return max(paths, key=key) if paths else None


def sdk_paths():
    sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    if not sdk:
        sys.exit("ANDROID_HOME is not set")
    bt = newest(glob.glob(os.path.join(sdk, "build-tools", "*")))
    plats = {int(m.group(1)): p for p in glob.glob(os.path.join(sdk, "platforms", "android-*"))
             if (m := re.fullmatch(r"android-(\d+)", os.path.basename(p)))}
    if not bt or not plats:
        sys.exit(f"{sdk}: no build-tools or platforms")
    plat = plats.get(TARGET_SDK) or plats[max(plats)]
    ndk = (os.environ.get("ANDROID_NDK_HOME") or os.environ.get("ANDROID_NDK_LATEST_HOME")
           or newest(glob.glob(os.path.join(sdk, "ndk", "*"))))
    if not ndk:
        sys.exit("no NDK: set ANDROID_NDK_HOME")
    host = "windows-x86_64" if WIN else ("darwin-x86_64" if sys.platform == "darwin" else "linux-x86_64")
    return sdk, bt, os.path.join(plat, "android.jar"), os.path.join(ndk, "toolchains", "llvm", "prebuilt", host, "bin")


def tool(bt, name):
    for ext in ([".exe", ".bat", ""] if WIN else [""]):
        p = os.path.join(bt, name + ext)
        if os.path.exists(p):
            return p
    sys.exit(f"{name} not found in {bt}")


def clang(ndkbin, prefix):
    return os.path.join(ndkbin, f"{prefix}{API}-clang" + (".cmd" if WIN else ""))


def version():
    toml = open(os.path.join(REPO, "Cargo.toml"), encoding="utf-8").read()
    m = re.search(r'^version\s*=\s*"([^"]+)"', toml, re.M)
    return m.group(1) if m else "0.0.0"


def build_modem(abi, ndkbin, target_dir):
    rust, prefix, _, _ = ABIS[abi]
    env = dict(os.environ)
    env[f"CARGO_TARGET_{rust.upper().replace('-', '_')}_LINKER"] = clang(ndkbin, prefix)
    env[f"CC_{rust.replace('-', '_')}"] = clang(ndkbin, prefix)
    env["CARGO_TARGET_DIR"] = target_dir
    run(["cargo", "build", "--release", "--locked", "--no-default-features", "-p", "xara", "--target", rust],
        cwd=REPO, env=env)
    out = os.path.join(target_dir, rust, "release")
    return {"libxara.so": os.path.join(out, "xara"), "libxaramonitor.so": os.path.join(out, "xara-monitor")}


def build_pat(abi, ndkbin):
    _, prefix, goarch, goarm = ABIS[abi]
    # outside the repo: Go's embed patterns reject paths with [ ] in them
    work = os.path.join(tempfile.gettempdir(), "xara-apk-go")
    os.makedirs(work, exist_ok=True)
    gopath = os.path.join(work, "gopath")
    env = dict(os.environ, GOOS="android", GOARCH=goarch, CGO_ENABLED="1", CC=clang(ndkbin, prefix),
               GOPATH=gopath, GOFLAGS="-trimpath")
    if goarm:
        env["GOARM"] = goarm
    env.pop("GOBIN", None)
    # GOOS=android with cgo: Pat resolves names through bionic, so its online features work on the phone too
    run(["go", "install", "-ldflags=-s -w", f"github.com/la5nta/pat@{PAT_VERSION}"], cwd=work, env=env)
    hits = glob.glob(os.path.join(gopath, "bin", f"android_{goarch}", "pat*"))
    if not hits:
        sys.exit(f"pat for {abi} not built")
    return hits[0], os.path.join(gopath, "pkg", "mod", "github.com", "la5nta", f"pat@{PAT_VERSION}", "LICENSE")


def test_commands(work):
    """The command-line rules, on the plain JVM (Commands.java has no Android types)."""
    out = os.path.join(work, "jvm-test")
    rmtree(out)
    run(["javac", "--release", "17", "-d", out,
         os.path.join(HERE, "src", "org", "xara", "app", "Commands.java"),
         os.path.join(HERE, "test", "CommandsTest.java")])
    run(["java", "-ea", "-cp", out, "CommandsTest"])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--abis", default=",".join(ABIS))
    ap.add_argument("--pat", action="store_true", help=f"bundle Pat {PAT_VERSION} (needs Go)")
    ap.add_argument("--out", default=os.path.join(REPO, "target", "android", "xara.apk"))
    ap.add_argument("--target-dir", default=os.path.join(REPO, "target"))
    args = ap.parse_args()
    abis = [a for a in args.abis.split(",") if a]
    for a in abis:
        if a not in ABIS:
            sys.exit(f"unknown ABI {a}; one of {', '.join(ABIS)}")

    sdk, bt, android_jar, ndkbin = sdk_paths()
    work = os.path.join(REPO, "target", "android", "build")
    rmtree(work)
    os.makedirs(work)
    test_commands(work)

    libs, pat_license = {}, None
    for abi in abis:
        libs[abi] = build_modem(abi, ndkbin, args.target_dir)
        if args.pat:
            libs[abi]["libpat.so"], pat_license = build_pat(abi, ndkbin)

    # resources and manifest -> base APK and R.java
    gen, classes, dex = (os.path.join(work, d) for d in ("gen", "classes", "dex"))
    for d in (gen, classes, dex):
        os.makedirs(d)
    res_zip = os.path.join(work, "res.zip")
    run([tool(bt, "aapt2"), "compile", "--dir", os.path.join(HERE, "res"), "-o", res_zip])
    base = os.path.join(work, "base.apk")
    ver = version()
    code = sum(int(x) * m for x, m in zip(ver.split(".")[:3], (10000, 100, 1)))
    run([tool(bt, "aapt2"), "link", "-o", base, "-I", android_jar, "--manifest", os.path.join(HERE, "AndroidManifest.xml"),
         "--min-sdk-version", API, "--target-sdk-version", TARGET_SDK, "--version-code", code, "--version-name", ver,
         "--java", gen, res_zip])

    srcs = files(os.path.join(HERE, "src"), ".java") + files(gen, ".java")
    run(["javac", "--release", "17", "-cp", android_jar, "-d", classes, *srcs])
    run([tool(bt, "d8"), "--release", "--min-api", API, "--lib", android_jar, "--output", dex,
         *files(classes, ".class")])

    unsigned = os.path.join(work, "unsigned.apk")
    shutil.copy(base, unsigned)
    with zipfile.ZipFile(unsigned, "a", zipfile.ZIP_DEFLATED) as z:
        z.write(os.path.join(dex, "classes.dex"), "classes.dex")
        for abi, bins in libs.items():
            for name, path in bins.items():
                z.write(path, f"lib/{abi}/{name}")
        z.write(os.path.join(REPO, "LICENSE"), "assets/LICENSE")
        if pat_license:
            z.write(pat_license, "assets/PAT-LICENSE")  # Pat is MIT: its notice travels with it

    aligned = os.path.join(work, "aligned.apk")
    run([tool(bt, "zipalign"), "-f", "-P", "16", "4", unsigned, aligned])
    ks = os.environ.get("XARA_KEYSTORE")
    ks_pass = os.environ.get("XARA_KEYSTORE_PASS", "android")
    alias = os.environ.get("XARA_KEY_ALIAS", "xara")
    if not ks:
        ks = os.path.join(REPO, "target", "android", "debug.keystore")
        if not os.path.exists(ks):
            run(["keytool", "-genkeypair", "-keystore", ks, "-storepass", ks_pass, "-keypass", ks_pass, "-alias", alias,
                 "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000", "-dname", "CN=xara debug"])
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    run([tool(bt, "apksigner"), "sign", "--ks", ks, "--ks-pass", f"pass:{ks_pass}", "--ks-key-alias", alias,
         "--out", args.out, aligned])
    run([tool(bt, "apksigner"), "verify", args.out])
    print(f"{args.out}: {os.path.getsize(args.out) / 1e6:.1f} MB, ABIs {', '.join(abis)}"
          + (f", Pat {PAT_VERSION}" if args.pat else ""))


if __name__ == "__main__":
    main()
