"""Real VARA HF (hf-a) with its clock at DATE calls TARGET for a few seconds over Voicemod, recorded by a passive
xara. Nothing reaches a radio. Usage: py -I run_c.py OUTDIR DATE TARGET"""
import os, socket, subprocess, sys, time, pathlib

HERE = pathlib.Path(__file__).parent
OUT = pathlib.Path(sys.argv[1]); OUT.mkdir(parents=True, exist_ok=True)
DATE, TARGET = sys.argv[2], sys.argv[3]
SHIM = HERE.parent / "clockshim" / "target" / "i686-pc-windows-msvc" / "release"
XARA = pathlib.Path(os.environ["XARA_EXE"])  # a plain xara build, used as a passive recorder
pid, rec = None, None
try:
    rec = subprocess.Popen([str(XARA), "--call", "N9REC", "--capture", "Voicemod", "--playback", "Voicemod", "--port", "8320",
                            "--kiss-port", "off", "--record", str(OUT / "air.wav")],
                           stdout=open(OUT / "rec.log", "w"), stderr=subprocess.STDOUT)
    d = HERE / "hf-a"
    env = dict(os.environ)
    if DATE != "real":
        env["CLOCKSHIM_DATE"] = DATE
        r = subprocess.run([str(SHIM / "runat.exe"), str(SHIM / "clockshim.dll"), str(d), str(d / "VARA.exe")],
                           env=env, capture_output=True, text=True)
        pid = int(r.stdout.strip())
    else:
        pid = subprocess.Popen([str(d / "VARA.exe")], cwd=d).pid
    time.sleep(4)
    for _ in range(60):
        try:
            s = socket.create_connection(("127.0.0.1", 8300), timeout=2)
            break
        except OSError:
            time.sleep(0.5)
    for c in ("MYCALL N0CALL", f"CONNECT N0CALL {TARGET}"):
        s.sendall(c.encode() + b"\r")
        time.sleep(0.5)
    time.sleep(12)
    s.sendall(b"ABORT\r")
    time.sleep(2)
finally:
    if pid:
        subprocess.run(["taskkill", "/PID", str(pid), "/F"], capture_output=True)
    if rec:
        rec.terminate()
        rec.wait(10)
