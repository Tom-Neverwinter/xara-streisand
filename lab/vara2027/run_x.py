"""Real VARA HF 4.9.1 (lab copy hf-a, clock shifted to DATE) against xara --tables 2027 over Voicemod.
Nothing reaches a radio. Usage: py -I run_x.py OUTDIR DATE BYTES vara-calls|xara-calls"""
import os, socket, subprocess, sys, time, pathlib, threading

HERE = pathlib.Path(__file__).parent
OUT = pathlib.Path(sys.argv[1]); OUT.mkdir(parents=True, exist_ok=True)
DATE, N, WHO = sys.argv[2], int(sys.argv[3]), sys.argv[4]
BW = sys.argv[5] if len(sys.argv) > 5 else "2300"
SHIM = HERE.parent / "clockshim" / "target" / "i686-pc-windows-msvc" / "release"
X27 = pathlib.Path(os.environ["XARA_EXE"])
VDIR = os.environ["XARA_VARA_DIR"]  # VARA folder whose .dat tables xara reads
pids, procs = [], []


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
    env = dict(os.environ, CLOCKSHIM_DATE=DATE)
    d = HERE / "hf-a"
    r = subprocess.run([str(SHIM / "runat.exe"), str(SHIM / "clockshim.dll"), str(d), str(d / "VARA.exe")],
                       env=env, capture_output=True, text=True)
    if r.returncode:
        raise SystemExit(f"runat: {r.stderr}")
    pids.append(int(r.stdout.strip()))
    x = subprocess.Popen([str(X27), "--call", "N1CALL", "--capture", "Voicemod", "--playback", "Voicemod", "--port", "8310",
                          "--kiss-port", "off", "--tables", "2027", "--vara-dir", VDIR, "--record", str(OUT / "xara.wav")],
                         stdout=open(OUT / "xara.log", "w"), stderr=subprocess.STDOUT)
    procs.append(x)
    print(f"VARA pid {pids[0]} ({DATE}), xara pid {x.pid}", flush=True)
    time.sleep(4)
    vara, xara = Port("vara", 8300), Port("xara", 8310)
    vara.send("MYCALL N0CALL"); xara.send("MYCALL N1CALL")
    vara.send("LISTEN ON"); xara.send("LISTEN ON")
    vara.send(f"BW{BW}"); xara.send(f"BW{BW}")
    time.sleep(2)
    caller, callee = (vara, xara) if WHO == "vara-calls" else (xara, vara)
    caller.send("CONNECT N0CALL N1CALL" if WHO == "vara-calls" else "CONNECT N1CALL N0CALL")
    linked = caller.wait(lambda: any(l.startswith("CONNECTED") for l in caller.lines), 90)
    print("linked:", linked, flush=True)
    if linked:
        callee.wait(lambda: any(l.startswith("CONNECTED") for l in callee.lines), 15)
        up = (b"2027 LAB CALLER " + bytes(range(65, 91)) * 400)[:N] + b"\r"
        down = (b"2027 LAB CALLEE " + bytes(range(97, 123)) * 40)[:600] + b"\r"
        caller.d.sendall(up); callee.d.sendall(down)
        caller.wait(lambda: callee.got.endswith(up) and caller.got.endswith(down), 150 + N // 8)
        print("caller->callee:", callee.got == up, len(callee.got), "/", len(up), flush=True)
        print("callee->caller:", caller.got == down, len(caller.got), "/", len(down), flush=True)
        caller.send("DISCONNECT")
        print("disconnected:", caller.wait(lambda: "DISCONNECTED" in caller.lines, 60), flush=True)
    else:
        caller.send("ABORT")
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
