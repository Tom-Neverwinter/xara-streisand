package org.xara.app;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.graphics.Typeface;
import android.media.AudioDeviceCallback;
import android.media.AudioDeviceInfo;
import android.media.AudioManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.view.View;
import android.view.ViewGroup;
import android.widget.AdapterView;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Spinner;
import android.widget.TextView;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.security.SecureRandom;
import java.util.ArrayList;
import java.util.List;

/** Settings, start/stop, the live log, and the way into the web UIs. */
public class MainActivity extends Activity {
    private SharedPreferences prefs;
    private Spinner role, band, capture, playback;
    private EditText call, patPassword, locator;
    private CheckBox transmit, pat;
    private TextView status, log;
    private Button start, stop, openUi, openPat;
    private final List<String> inIds = new ArrayList<>(), outIds = new ArrayList<>();
    private final Handler handler = new Handler(Looper.getMainLooper());
    private AudioManager audio;
    private boolean havePat;

    private final Runnable tick = new Runnable() {
        @Override
        public void run() {
            refresh();
            handler.postDelayed(this, 1000);
        }
    };

    private final AudioDeviceCallback devicesChanged = new AudioDeviceCallback() {
        @Override
        public void onAudioDevicesAdded(AudioDeviceInfo[] added) {
            listDevices();
        }

        @Override
        public void onAudioDevicesRemoved(AudioDeviceInfo[] removed) {
            listDevices();
        }
    };

    private String libDir() {
        return getApplicationInfo().nativeLibraryDir;
    }

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        prefs = getSharedPreferences("xara", MODE_PRIVATE);
        audio = (AudioManager) getSystemService(AUDIO_SERVICE);
        havePat = new File(libDir(), "libpat.so").exists();

        LinearLayout col = new LinearLayout(this);
        col.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(16);
        col.setPadding(pad, pad, pad, pad);

        TextView title = new TextView(this);
        title.setText("xara: a VARA-compatible modem");
        title.setTextSize(20);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        col.addView(title);

        role = spinner(col, "Role", new String[] {"Monitor (receive only)", "TNC for Pat / Winlink (transmits)"});
        band = spinner(col, "Mode", Commands.BAND_NAMES);
        call = edit(col, "Callsign", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS);
        capture = spinner(col, "Audio from the radio", new String[] {"Default input"});
        playback = spinner(col, "Audio to the radio", new String[] {"Default output"});

        transmit = new CheckBox(this);
        transmit.setText("I am licensed: transmit (VOX keys the radio)");
        col.addView(transmit);

        pat = new CheckBox(this);
        pat.setText(havePat ? "Run Pat (Winlink client) on this phone" : "Pat is not in this build");
        pat.setEnabled(havePat);
        col.addView(pat);
        patPassword = edit(col, "Winlink password (Pat)", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        locator = edit(col, "Grid locator (Pat, optional)", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS);

        LinearLayout row = new LinearLayout(this);
        start = button(row, "Start", v -> start());
        stop = button(row, "Stop", v -> startService(new Intent(this, XaraService.class).setAction(XaraService.ACTION_STOP)));
        col.addView(row);
        LinearLayout row2 = new LinearLayout(this);
        openUi = button(row2, "Modem UI", v -> open(Commands.uiUrl(settings())));
        openPat = button(row2, "Pat", v -> open("http://127.0.0.1:" + Commands.PAT_PORT + "/"));
        col.addView(row2);

        status = new TextView(this);
        status.setPadding(0, dp(8), 0, dp(8));
        col.addView(status);
        log = new TextView(this);
        log.setTypeface(Typeface.MONOSPACE);
        log.setTextSize(11);
        log.setTextIsSelectable(true);
        col.addView(log);

        ScrollView scroll = new ScrollView(this);
        scroll.addView(col);
        setContentView(scroll);

        load();
        role.setOnItemSelectedListener(new AdapterView.OnItemSelectedListener() {
            @Override
            public void onItemSelected(AdapterView<?> p, View v, int pos, long id) {
                refresh();
            }

            @Override
            public void onNothingSelected(AdapterView<?> p) {}
        });
        listDevices();
        audio.registerAudioDeviceCallback(devicesChanged, handler);
        askPermissions();
    }

    @Override
    protected void onResume() {
        super.onResume();
        handler.post(tick);
    }

    @Override
    protected void onPause() {
        handler.removeCallbacks(tick);
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        audio.unregisterAudioDeviceCallback(devicesChanged);
        super.onDestroy();
    }

    private void askPermissions() {
        List<String> want = new ArrayList<>();
        want.add(Manifest.permission.RECORD_AUDIO);
        if (Build.VERSION.SDK_INT >= 33) {
            want.add(Manifest.permission.POST_NOTIFICATIONS);
        }
        List<String> missing = new ArrayList<>();
        for (String p : want) {
            if (checkSelfPermission(p) != PackageManager.PERMISSION_GRANTED) {
                missing.add(p);
            }
        }
        if (!missing.isEmpty()) {
            requestPermissions(missing.toArray(new String[0]), 1);
        }
    }

    /** Audio devices with the ids AAudio takes; USB sound cards are the usual radio interface. */
    private void listDevices() {
        fill(capture, inIds, audio.getDevices(AudioManager.GET_DEVICES_INPUTS), "Default input", prefs.getString("capture", "default"));
        fill(playback, outIds, audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS), "Default output", prefs.getString("playback", "default"));
    }

    private void fill(Spinner s, List<String> ids, AudioDeviceInfo[] devs, String first, String want) {
        List<String> names = new ArrayList<>();
        ids.clear();
        names.add(first);
        ids.add("default");
        for (AudioDeviceInfo d : devs) {
            names.add(kind(d.getType()) + ": " + d.getProductName() + " (" + d.getId() + ")");
            ids.add(String.valueOf(d.getId()));
        }
        ArrayAdapter<String> a = new ArrayAdapter<>(this, android.R.layout.simple_spinner_item, names);
        a.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        s.setAdapter(a);
        s.setSelection(Math.max(0, ids.indexOf(want)));
    }

    private static String kind(int t) {
        switch (t) {
            case AudioDeviceInfo.TYPE_USB_DEVICE:
            case AudioDeviceInfo.TYPE_USB_HEADSET:
            case AudioDeviceInfo.TYPE_USB_ACCESSORY:
                return "USB";
            case AudioDeviceInfo.TYPE_BUILTIN_MIC:
                return "Built-in mic";
            case AudioDeviceInfo.TYPE_BUILTIN_SPEAKER:
                return "Speaker";
            case AudioDeviceInfo.TYPE_BUILTIN_EARPIECE:
                return "Earpiece";
            case AudioDeviceInfo.TYPE_WIRED_HEADSET:
            case AudioDeviceInfo.TYPE_WIRED_HEADPHONES:
                return "Wired";
            case AudioDeviceInfo.TYPE_BLUETOOTH_SCO:
            case AudioDeviceInfo.TYPE_BLUETOOTH_A2DP:
                return "Bluetooth";
            default:
                return "Device";
        }
    }

    private Commands.Settings settings() {
        Commands.Settings s = new Commands.Settings();
        s.role = role.getSelectedItemPosition() == 1 ? Commands.TNC : Commands.MONITOR;
        s.band = Commands.BANDS[Math.max(0, band.getSelectedItemPosition())];
        s.call = call.getText().toString().trim().toUpperCase(java.util.Locale.ROOT);
        s.capture = inIds.get(Math.max(0, capture.getSelectedItemPosition()));
        s.playback = outIds.get(Math.max(0, playback.getSelectedItemPosition()));
        s.transmit = transmit.isChecked();
        // the Pat box is greyed out outside the TNC role, so a tick left from before does not count there
        s.pat = Commands.TNC.equals(s.role) && pat.isChecked();
        s.patPassword = patPassword.getText().toString();
        s.locator = locator.getText().toString().trim().toUpperCase(java.util.Locale.ROOT);
        s.token = prefs.getString("token", "");
        return s;
    }

    private void load() {
        role.setSelection(prefs.getString("role", Commands.MONITOR).equals(Commands.TNC) ? 1 : 0);
        band.setSelection(Math.max(0, Commands.indexOf(prefs.getString("band", "2300"))));
        call.setText(prefs.getString("call", ""));
        pat.setChecked(havePat && prefs.getBoolean("pat", false));
        patPassword.setText(prefs.getString("patPassword", ""));
        locator.setText(prefs.getString("locator", ""));
        // transmitting is confirmed afresh each time the app opens
        transmit.setChecked(false);
    }

    private void save(Commands.Settings s) {
        prefs.edit()
            .putString("role", s.role).putString("band", s.band).putString("call", s.call)
            .putString("capture", s.capture).putString("playback", s.playback)
            .putBoolean("pat", pat.isChecked()).putBoolean("patRan", s.pat).putString("patPassword", s.patPassword).putString("locator", s.locator)
            .putString("token", s.token)
            .apply();
    }

    private void start() {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            askPermissions();
            status.setText("xara needs the microphone permission to hear the radio.");
            return;
        }
        Commands.Settings s = settings();
        String why = Commands.problem(s, havePat);
        if (why != null) {
            status.setText(why);
            return;
        }
        byte[] t = new byte[16];
        new SecureRandom().nextBytes(t);
        StringBuilder hex = new StringBuilder();
        for (byte b : t) {
            hex.append(String.format("%02x", b));
        }
        s.token = hex.toString();
        save(s);
        ArrayList<String> cmds = new ArrayList<>();
        cmds.add(String.join("\0", Commands.modem(s, libDir())));
        if (s.pat) {
            File cfg = new File(getFilesDir(), "pat-config.json");
            try (FileOutputStream o = new FileOutputStream(cfg)) {
                o.write(Commands.patConfig(s).getBytes(StandardCharsets.UTF_8));
            } catch (IOException e) {
                status.setText("Could not write Pat's config: " + e.getMessage());
                return;
            }
            cmds.add(String.join("\0", Commands.pat(libDir(), cfg.getAbsolutePath())));
        }
        String summary = (Commands.TNC.equals(s.role) ? "TNC " : "Monitor ") + Commands.BAND_NAMES[Commands.indexOf(s.band)];
        Intent i = new Intent(this, XaraService.class).setAction(XaraService.ACTION_START)
            .putStringArrayListExtra(XaraService.EXTRA_COMMANDS, cmds)
            .putExtra(XaraService.EXTRA_SUMMARY, summary);
        if (Build.VERSION.SDK_INT >= 26) {
            startForegroundService(i);
        } else {
            startService(i);
        }
        handler.postDelayed(this::refresh, 300);
    }

    private void open(String url) {
        startActivity(new Intent(this, WebActivity.class).putExtra(WebActivity.EXTRA_URL, url));
    }

    private void refresh() {
        boolean on = XaraService.running;
        boolean tnc = role.getSelectedItemPosition() == 1;
        start.setEnabled(!on);
        stop.setEnabled(on);
        openUi.setEnabled(on);
        openPat.setEnabled(on && prefs.getBoolean("patRan", false));
        transmit.setEnabled(tnc && !on);
        playback.setEnabled(tnc && !on);
        pat.setEnabled(havePat && tnc && !on);
        boolean ranTnc = Commands.TNC.equals(prefs.getString("role", Commands.MONITOR));
        if (on && ranTnc) {
            status.setText("Running. Pat or another client connects to 127.0.0.1:" + Commands.CMD_PORT
                + " (KISS " + Commands.KISS_PORT + ").");
        } else if (on) {
            status.setText("Listening. \"Modem UI\" shows what is heard.");
        } else if (status.getText().length() == 0 || status.getText().toString().startsWith("Running")) {
            status.setText(tnc ? "Stopped. The TNC transmits through VOX when a client connects."
                : "Stopped. The monitor only listens.");
        }
        log.setText(tail(XaraService.logFile(this), 6000));
    }

    private static String tail(File f, int max) {
        if (!f.exists()) {
            return "";
        }
        try (RandomAccessFile r = new RandomAccessFile(f, "r")) {
            long n = r.length();
            long from = Math.max(0, n - max);
            byte[] b = new byte[(int) (n - from)];
            r.seek(from);
            r.readFully(b);
            return new String(b, StandardCharsets.UTF_8);
        } catch (IOException e) {
            return e.getMessage();
        }
    }

    private Spinner spinner(LinearLayout col, String label, String[] items) {
        label(col, label);
        Spinner s = new Spinner(this);
        ArrayAdapter<String> a = new ArrayAdapter<>(this, android.R.layout.simple_spinner_item, items);
        a.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        s.setAdapter(a);
        col.addView(s);
        return s;
    }

    private EditText edit(LinearLayout col, String label, int type) {
        label(col, label);
        EditText e = new EditText(this);
        e.setInputType(type);
        e.setSingleLine(true);
        col.addView(e);
        return e;
    }

    private void label(LinearLayout col, String text) {
        TextView t = new TextView(this);
        t.setText(text);
        t.setPadding(0, dp(10), 0, 0);
        col.addView(t);
    }

    private Button button(LinearLayout row, String text, View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(text);
        b.setOnClickListener(l);
        row.addView(b, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        return b;
    }

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }
}
