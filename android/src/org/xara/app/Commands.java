package org.xara.app;

import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

/**
 * The command lines the service runs, from the user's settings. Plain Java (no Android types) so the rules here
 * are tested on the JVM by build_apk.py: receive-only unless transmitting was asked for, one band flag, the web
 * UIs and Pat on localhost only.
 */
public final class Commands {
    public static final String MONITOR = "monitor";
    public static final String TNC = "tnc";
    public static final String[] BANDS = {"2300", "500", "2750", "sat", "fm-narrow", "fm-wide"};
    public static final String[] BAND_NAMES = {
        "HF 2300 Hz", "HF 500 Hz", "HF 2750 Hz", "VARA SAT", "FM narrow (12.5 kHz)", "FM wide (25 kHz)"};
    public static final int CMD_PORT = 8300;
    public static final int KISS_PORT = 8100;
    public static final int UI_PORT = 8088;
    public static final int PAT_PORT = 8080;
    // xara's rule (xara-host valid_call): 1-7 letters or digits, then an optional -T/R/X/L, -1..-9 or -01..-19
    private static final Pattern CALL = Pattern.compile("^[A-Z0-9]{1,7}(-([TRXL1-9]|0[1-9]|1[0-9]))?$");

    public static final class Settings {
        public String role = MONITOR;
        public String band = "2300";
        public String call = "";
        public String capture = "default";
        public String playback = "default";
        public boolean transmit;
        public boolean pat;
        public String patPassword = "";
        public String locator = "";
        public String token = "";
    }

    private Commands() {}

    public static boolean validCall(String c) {
        // the TNC refuses the placeholder N0CALL, as VARA does
        return CALL.matcher(c).matches() && !c.split("-")[0].equals("N0CALL");
    }

    /** Why these settings can't start, or null. */
    public static String problem(Settings s, boolean havePat) {
        boolean tnc = TNC.equals(s.role);
        if (tnc && !validCall(s.call)) {
            return "Enter your callsign (e.g. N0CALL or N0CALL-7).";
        }
        if (!tnc && !s.call.isEmpty() && !validCall(s.call)) {
            return "\"" + s.call + "\" is not a callsign.";
        }
        if (tnc && !s.transmit) {
            return "The TNC transmits: tick \"I am licensed: transmit\" first, or use the monitor (receive only).";
        }
        if (s.pat && !tnc) {
            return "Pat needs the TNC role.";
        }
        if (s.pat && !havePat) {
            return "This build has no Pat.";
        }
        if (indexOf(s.band) < 0) {
            return "Unknown band " + s.band + ".";
        }
        return null;
    }

    static int indexOf(String band) {
        for (int i = 0; i < BANDS.length; i++) {
            if (BANDS[i].equals(band)) {
                return i;
            }
        }
        return -1;
    }

    private static void band(List<String> a, String band) {
        if (band.startsWith("fm-")) {
            a.add("--fm");
            a.add(band.substring(3));
        } else {
            a.add("--hf");
            a.add(band);
        }
    }

    /** The modem's argv; `libDir` holds the executables. */
    public static List<String> modem(Settings s, String libDir) {
        List<String> a = new ArrayList<>();
        boolean tnc = TNC.equals(s.role);
        a.add(libDir + (tnc ? "/libxara.so" : "/libxaramonitor.so"));
        a.add("--audio");
        a.add("aaudio");
        a.add("--capture");
        a.add(s.capture);
        band(a, s.band);
        if (tnc) {
            a.add("--call");
            a.add(s.call);
            a.add("--playback");
            a.add(s.playback);
            a.add("--radio");
            a.add("--ptt");
            a.add("vox");
            a.add("--port");
            a.add(String.valueOf(CMD_PORT));
            a.add("--kiss-port");
            a.add(String.valueOf(KISS_PORT));
            a.add("--ui-port");
            a.add(String.valueOf(UI_PORT));
            a.add("--ui-control");
            a.add("--ui-token");
            a.add(s.token);
        } else {
            if (!s.call.isEmpty()) {
                a.add("--call");
                a.add(s.call);
            }
            a.add("--ui-port");
            a.add(String.valueOf(UI_PORT));
        }
        return a;
    }

    /** Pat's argv: its web UI, with the config `patConfig` wrote. */
    public static List<String> pat(String libDir, String configPath) {
        List<String> a = new ArrayList<>();
        a.add(libDir + "/libpat.so");
        a.add("--config");
        a.add(configPath);
        a.add("http");
        a.add("--addr");
        a.add("127.0.0.1:" + PAT_PORT);
        return a;
    }

    /** Pat's config.json: both VARA transports pointed at the TNC on localhost. */
    public static String patConfig(Settings s) {
        int bw = s.band.startsWith("fm-") || "sat".equals(s.band) ? 2300 : Integer.parseInt(s.band);
        return "{\n"
            + "  \"mycall\": " + json(s.call) + ",\n"
            + "  \"secure_login_password\": " + json(s.patPassword) + ",\n"
            + "  \"locator\": " + json(s.locator) + ",\n"
            + "  \"http_addr\": \"127.0.0.1:" + PAT_PORT + "\",\n"
            + "  \"varahf\": {\"addr\": \"127.0.0.1:" + CMD_PORT + "\", \"bandwidth\": " + bw
            + ", \"rig\": \"\", \"ptt_ctrl\": false},\n"
            + "  \"varafm\": {\"addr\": \"127.0.0.1:" + CMD_PORT + "\", \"rig\": \"\", \"ptt_ctrl\": false}\n"
            + "}\n";
    }

    /** The URL of the modem's web UI (the TNC's needs its control token). */
    public static String uiUrl(Settings s) {
        return "http://127.0.0.1:" + UI_PORT + "/" + (TNC.equals(s.role) ? "?token=" + s.token : "");
    }

    static String json(String v) {
        StringBuilder b = new StringBuilder("\"");
        for (char c : v.toCharArray()) {
            switch (c) {
                case '"': b.append("\\\""); break;
                case '\\': b.append("\\\\"); break;
                case '\n': b.append("\\n"); break;
                case '\r': b.append("\\r"); break;
                case '\t': b.append("\\t"); break;
                default:
                    if (c < 0x20) {
                        b.append(String.format("\\u%04x", (int) c));
                    } else {
                        b.append(c);
                    }
            }
        }
        return b.append('"').toString();
    }
}
