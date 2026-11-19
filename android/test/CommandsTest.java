import java.util.List;
import org.xara.app.Commands;
import org.xara.app.Commands.Settings;

/** The app's command-line rules, run on the plain JVM by build_apk.py (java -ea). */
public class CommandsTest {
    static void check(boolean ok, String what) {
        if (!ok) {
            throw new AssertionError(what);
        }
    }

    static Settings tnc() {
        Settings s = new Settings();
        s.role = Commands.TNC;
        s.call = "W1AW";
        s.transmit = true;
        s.token = "t0k";
        return s;
    }

    public static void main(String[] a) {
        // calls: xara's rule, N0CALL refused
        for (String c : new String[] {"W1AW", "N0CALL-7".replace("N0CALL", "K1ABC"), "VK2ABC-T", "G4XYZ-15", "3DA0RU"}) {
            check(Commands.validCall(c), c);
        }
        for (String c : new String[] {"", "N0CALL", "N0CALL-7", "W1AW-20", "W1AW-0", "TOOLONGCALL", "W1 AW", "w1aw"}) {
            check(!Commands.validCall(c), c);
        }

        // receive only unless transmitting was confirmed; the TNC needs a call
        Settings mon = new Settings();
        check(Commands.problem(mon, false) == null, "monitor without a call");
        List<String> m = Commands.modem(mon, "/lib");
        check(m.get(0).equals("/lib/libxaramonitor.so"), "monitor binary");
        check(!m.contains("--radio") && !m.contains("--playback"), "monitor never transmits");
        check(m.containsAll(List.of("--audio", "aaudio", "--hf", "2300", "--ui-port")), "monitor args " + m);

        Settings s = tnc();
        s.transmit = false;
        check(Commands.problem(s, false).contains("licensed"), "TNC without consent");
        s = tnc();
        s.call = "";
        check(Commands.problem(s, false).contains("callsign"), "TNC without call");
        s = tnc();
        check(Commands.problem(s, false) == null, "TNC ok");
        List<String> t = Commands.modem(s, "/lib");
        check(t.get(0).equals("/lib/libxara.so"), "TNC binary");
        check(t.containsAll(List.of("--call", "W1AW", "--radio", "--ptt", "vox", "--ui-control", "--ui-token", "t0k")),
            "TNC args " + t);
        check(Commands.uiUrl(s).endsWith("?token=t0k"), "TNC UI url carries the token");

        // one band flag: --fm for FM, --hf otherwise
        s.band = "fm-wide";
        t = Commands.modem(s, "/lib");
        check(t.contains("--fm") && t.contains("wide") && !t.contains("--hf"), "FM " + t);
        s.band = "sat";
        t = Commands.modem(s, "/lib");
        check(t.contains("--hf") && t.contains("sat"), "SAT " + t);

        // Pat: TNC only, present only when bundled, config points at the TNC and escapes what it's given
        s = tnc();
        s.pat = true;
        check(Commands.problem(s, false).contains("no Pat"), "Pat missing");
        check(Commands.problem(s, true) == null, "Pat ok");
        mon.pat = true;
        check(Commands.problem(mon, true).contains("TNC"), "Pat needs the TNC");
        s.patPassword = "a\"b\\c";
        s.band = "500";
        String cfg = Commands.patConfig(s);
        check(cfg.contains("\"secure_login_password\": \"a\\\"b\\\\c\""), cfg);
        check(cfg.contains("\"addr\": \"127.0.0.1:8300\", \"bandwidth\": 500"), cfg);
        check(cfg.contains("\"http_addr\": \"127.0.0.1:8080\""), cfg);
        check(Commands.pat("/lib", "/cfg").equals(List.of("/lib/libpat.so", "--config", "/cfg", "http", "--addr", "127.0.0.1:8080")),
            "pat argv");
        System.out.println("CommandsTest: ok");
    }
}
