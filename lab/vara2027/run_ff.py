"""Two real VARA FM 4.4.1 stations (lab copies fm-a, fm-b) linked over Voicemod with their clock shifted to DATE,
recorded by a passive xara. Nothing reaches a radio. Usage: py -I run.py OUTDIR DATE|real [BYTES]"""
import os, socket, subprocess, sys, time, pathlib, threading

HERE = pathlib.Path(__file__).parent
OUT = pathlib.Path(sys.argv[1]); OUT.mkdir(parents=True, exist_ok=True)
DATE = sys.argv[2]
N = int(sys.argv[3]) if len(sys.argv) > 3 else 1500
SHIM = HERE.parent / "clockshim" / "target" / "i686-pc-windows-msvc" / "release"
XARA = pathlib.Path(os.environ["XARA_EXE"])  # a plain xara build, used as a passive recorder
pids, procs = [], []


def vara(name):
    d = HERE / name
    env = dict(os.environ)
    if DATE != "real":
        env["CLOCKSHIM_DATE"] = DATE
        out = subprocess.run([str(SHIM / "runat.exe"), str(SHIM / "clockshim.dll"), str(d), str(d / "VARAFM.exe")],
                             env=env, capture_output=True, text=True)
        if out.returncode:
            raise SystemExit(f"runat {name}: {out.stderr}")
        pid = int(out.stdout.strip())
    else:
        pid = subprocess.Popen([str(d / "VARAFM.exe")], cwd=d).pid
    pids.append(pid)
    print(f"{name} pid {pid} ({DATE})", flush=True)


class Port:
    def __init__(self, name, port):
        self.log = open(OUT / f"{name}.cmd.log", "w")
        for _ in range(60):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=2)
                self.d = socket.create_connection(("127.0.0.1", port + 1), timeout=2)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise SystemExit(f"no port {port}")
        self.lines, self.got, self.t0 = [], b"", time.time()
        threading.Thread(target=self.read, daemon=True).start()
        threading.Thread(target=self.read_data, daemon=True).start()

    def read(self):
        buf = b""
        while True:
            try:
                d = self.s.recv(4096)
            except OSError:
                continue
            if not d:
                return
            buf += d
            *done, buf = buf.split(b"\r")
            for l in done:
                t = l.decode("latin-1")
                self.lines.append(t)
                self.log.write(f"{time.time() - self.t0:7.2f} < {t}\n"); self.log.flush()

    def read_data(self):
        while True:
            try:
                d = self.d.recv(4096)
            except OSError:
                continue
            if not d:
                return
            self.got += d

    def send(self, cmd):
        self.log.write(f"{time.time() - self.t0:7.2f} > {cmd}\n"); self.log.flush()
        self.s.sendall(cmd.encode() + b"\r")

    def wait(self, pred, secs):
        end = time.time() + secs
        while time.time() < end:
            if pred():
                return True
            time.sleep(0.2)
        return False


try:
    rec = subprocess.Popen([str(XARA), "--call", "N9REC", "--capture", "Voicemod", "--playback", "Voicemod", "--port", "8320",
                            "--kiss-port", "off", "--record", str(OUT / "air.wav")],
                           stdout=open(OUT / "rec.log", "w"), stderr=subprocess.STDOUT)
    procs.append(rec)
    vara("fm-a")
    vara("fm-b")
    time.sleep(4)
    a, b = Port("a", 8300), Port("b", 8310)
    a.send("MYCALL N0CALL"); b.send("MYCALL N1CALL")
    a.send("LISTEN ON"); b.send("LISTEN ON")
    time.sleep(2)
    a.send("CONNECT N0CALL N1CALL")
    linked = a.wait(lambda: any(l.startswith("CONNECTED") for l in a.lines), 90)
    print("linked:", linked, flush=True)
    if linked:
        b.wait(lambda: any(l.startswith("CONNECTED") for l in b.lines), 15)
        up = (b"2027 LAB A->B " + bytes(range(65, 91)) * 200)[:N] + b"\r"
        down = (b"2027 LAB B->A " + bytes(range(97, 123)) * 20)[:300] + b"\r"
        a.d.sendall(up); b.d.sendall(down)
        a.wait(lambda: b.got.endswith(up) and a.got.endswith(down), 120 + N // 10)
        print("A->B:", b.got == up, len(b.got), "/", len(up), " B->A:", a.got == down, len(a.got), "/", len(down), flush=True)
        a.send("DISCONNECT")
        print("disconnected:", a.wait(lambda: "DISCONNECTED" in a.lines, 60), flush=True)
    else:
        a.send("ABORT")
    time.sleep(3)
finally:
    for pid in pids:
        subprocess.run(["taskkill", "/PID", str(pid), "/F"], capture_output=True)
    for p in procs:
        p.terminate()
        try:
            p.wait(10)
        except subprocess.TimeoutExpired:
            p.kill()
    for f in ("a.cmd.log", "b.cmd.log"):
        print(f"== {f}"); print((OUT / f).read_text(errors="replace")[-1500:])
