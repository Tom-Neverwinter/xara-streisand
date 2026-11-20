# Lab: real VARA in its 2027 mode

No 2027-format VARA traffic exists on air before 2027-01-01, so xara's `--tables 2027` support was built and checked
against real VARA running with its clock moved forward, in this lab.

`clockshim/` makes one 32-bit Windows process see another date (`CLOCKSHIM_DATE=YYYY-MM-DD`), leaving the machine
clock and every timer alone: `runat.exe SHIM.dll DIR PROGRAM` starts PROGRAM suspended, loads the shim (which
patches kernelbase's GetSystemTime*/GetLocalTime), then lets it run. Build:
`cargo build --release --target i686-pc-windows-msvc` in `clockshim/`.

`vara2027/` drives copies of your own VARA HF / FM folders over a virtual audio cable, recorded by a passive xara;
nothing reaches a radio. Put the copies next to the scripts as `hf-a`, `hf-b` (HF) and `fm-a`, `fm-b` (FM), each set
to the virtual cable, with command ports 8300 (`-a`) and 8310 (`-b`). Set `XARA_EXE` to a xara build and, for
`run_x.py` / `run_fm.py`, `XARA_VARA_DIR` to the VARA folder xara reads its 2027 tables from. The scripts use the
device name `Voicemod`; change it to your cable's name.

- `run.py` / `run_ff.py`: two VARA HF / FM stations, clock at DATE, link and exchange text
- `run_c.py`: one VARA HF calls a target at DATE (to learn the connect key blind)
- `run_x.py` / `run_fm.py`: VARA (HF / FM) at DATE against `xara --tables 2027`, either side calling

Findings (2026-11-19): with the clock in 2027 VARA HF 4.9.1 and FM 4.4.1 use their 2027 tables and seed their
control codebooks with shifted call keys (HF +687, FM +933, the same for 2027 and 2028 dates); xara `--tables 2027`
links with both: HF 500/2300/2750 up to level 13/14/15, FM narrow and wide.
