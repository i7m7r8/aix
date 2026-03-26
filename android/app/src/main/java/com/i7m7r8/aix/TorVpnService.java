package com.i7m7r8.aix;

import android.net.VpnService;
import android.os.ParcelFileDescriptor;
import android.util.Log;
import android.system.OsConstants;

public class TorVpnService extends VpnService {
    private static final String TAG = "AIX_VPN";
    private ParcelFileDescriptor vpnInterface = null;

    @Override
    public int onStartCommand(android.content.Intent intent, int flags, int startId) {
        Log.i(TAG, "Starting AIX Tor VPN");
        try {
            // Retrieve SNI and bridge config from intent extras
            String sni = intent.getStringExtra("sni");
            String bridge = intent.getStringExtra("bridge");
            if (sni != null) Log.i(TAG, "Using SNI: " + sni);
            if (bridge != null) Log.i(TAG, "Using bridge: " + bridge);

            Builder builder = new Builder();
            builder.setSession("AIX Tor VPN with Custom SNI")
                   .addAddress("10.0.0.2", 32)
                   .addDnsServer("8.8.8.8")
                   .addRoute("0.0.0.0", 0)
                   .addRoute("::", 0)                     // IPv6 route
                   .setMtu(1500)
                   .allowFamily(OsConstants.AF_INET)
                   .allowFamily(OsConstants.AF_INET6);    // allow IPv6

            vpnInterface = builder.establish();
            if (vpnInterface != null) {
                int fd = vpnInterface.getFd();
                // Pass the config to native code
                startTorWithTun(fd, sni, bridge);
            }
        } catch (Exception e) {
            Log.e(TAG, "VPN start failed", e);
            stopSelf();   // don't keep restarting
        }
        return START_STICKY;
    }

    private native void startTorWithTun(int tunFd, String sni, String bridge);
    private native void stopTorNative();

    @Override
    public void onDestroy() {
        stopTorNative();
        if (vpnInterface != null) {
            try { vpnInterface.close(); } catch (Exception ignored) {}
        }
        super.onDestroy();
    }
}
