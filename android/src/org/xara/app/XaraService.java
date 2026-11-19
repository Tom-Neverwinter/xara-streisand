package org.xara.app;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.os.PowerManager;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

/**
 * Runs the modem (and Pat, when asked) as child processes for as long as the user wants, screen off included:
 * a foreground service of type microphone, so Android keeps the capture stream alive, plus a partial wake lock.
 * Each process's output goes to a log file the activity shows.
 */
public class XaraService extends Service {
    static final String ACTION_START = "org.xara.app.START";
    static final String ACTION_STOP = "org.xara.app.STOP";
    static final String EXTRA_COMMANDS = "commands"; // ArrayList<String>: one "\0"-joined argv per process
    static final String EXTRA_SUMMARY = "summary";
    private static final String CHANNEL = "xara";
    private static final int NOTIFICATION = 1;

    static volatile boolean running;
    private final List<Process> procs = new ArrayList<>();
    private PowerManager.WakeLock wake;

    static File logFile(android.content.Context c) {
        return new File(c.getFilesDir(), "xara.log");
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null || ACTION_STOP.equals(intent.getAction())) {
            stopSelf();
            return START_NOT_STICKY;
        }
        if (running) {
            return START_NOT_STICKY;
        }
        String summary = intent.getStringExtra(EXTRA_SUMMARY);
        Notification n = notification(summary == null ? "running" : summary);
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE);
        } else {
            startForeground(NOTIFICATION, n);
        }
        PowerManager pm = (PowerManager) getSystemService(POWER_SERVICE);
        wake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "xara:modem");
        wake.acquire();
        running = true;
        File log = logFile(this);
        log.delete();
        ArrayList<String> commands = intent.getStringArrayListExtra(EXTRA_COMMANDS);
        if (commands != null) {
            for (String c : commands) {
                start(c.split("\0"), log);
            }
        }
        return START_NOT_STICKY;
    }

    private void start(String[] argv, File log) {
        String name = new File(argv[0]).getName().replaceFirst("^lib", "").replaceFirst("\\.so$", "");
        try {
            ProcessBuilder pb = new ProcessBuilder(argv).directory(getFilesDir()).redirectErrorStream(true);
            // Pat keeps its mailbox and config under HOME
            pb.environment().put("HOME", getFilesDir().getAbsolutePath());
            Process p = pb.start();
            synchronized (procs) {
                procs.add(p);
            }
            // the web UI's control token stays out of the log (it is on screen, and in any screenshot of it)
            String shown = String.join(" ", argv).replaceAll("(--ui-token )\\S+", "$1<token>");
            append(log, "[" + name + "] started: " + shown + "\n");
            Thread t = new Thread(() -> pump(p, name, log), "log-" + name);
            t.setDaemon(true);
            t.start();
        } catch (IOException e) {
            append(log, "[" + name + "] failed to start: " + e.getMessage() + "\n");
        }
    }

    private void pump(Process p, String name, File log) {
        try (BufferedReader r = new BufferedReader(new InputStreamReader(p.getInputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = r.readLine()) != null) {
                append(log, "[" + name + "] " + line.replaceAll("(token=)[0-9A-Za-z]+", "$1<token>") + "\n");
            }
            append(log, "[" + name + "] exited with " + p.waitFor() + "\n");
        } catch (IOException | InterruptedException e) {
            append(log, "[" + name + "] " + e.getMessage() + "\n");
        }
    }

    private static synchronized void append(File log, String s) {
        // keep the log bounded: start over past 2 MB
        try (FileOutputStream o = new FileOutputStream(log, log.length() < 2_000_000)) {
            o.write(s.getBytes(StandardCharsets.UTF_8));
        } catch (IOException ignored) {
        }
    }

    private Notification notification(String text) {
        NotificationManager nm = getSystemService(NotificationManager.class);
        if (Build.VERSION.SDK_INT >= 26) {
            nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Modem", NotificationManager.IMPORTANCE_LOW));
        }
        int imm = Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0;
        PendingIntent open = PendingIntent.getActivity(this, 0, new Intent(this, MainActivity.class), imm);
        PendingIntent stop = PendingIntent.getService(this, 1, new Intent(this, XaraService.class).setAction(ACTION_STOP), imm);
        Notification.Builder b = Build.VERSION.SDK_INT >= 26 ? new Notification.Builder(this, CHANNEL) : new Notification.Builder(this);
        return b.setContentTitle("xara")
                .setContentText(text)
                .setSmallIcon(R.drawable.ic_xara)
                .setOngoing(true)
                .setContentIntent(open)
                .addAction(new Notification.Action.Builder(null, "Stop", stop).build())
                .build();
    }

    @Override
    public void onDestroy() {
        synchronized (procs) {
            for (Process p : procs) {
                p.destroy();
            }
            procs.clear();
        }
        if (wake != null && wake.isHeld()) {
            wake.release();
        }
        running = false;
        append(logFile(this), "[app] stopped\n");
        super.onDestroy();
    }
}
