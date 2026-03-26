package com.i7m7r8.aix;

import android.net.VpnService;
import android.os.ParcelFileDescriptor;
import android.util.Log;

public class TorVpnService extends VpnService {
    private static final String TAG = "AIX_VPN";
    private ParcelFileDescriptor vpnInterface = null;

    @Override
    public int onStartCommand(android.content.Intent intent, int flags, int startId) {
        Log.i(TAG, "Starting AIX VPN Service");
        try {
            Builder builder = new Builder();
            builder.setSession("AIX Tor VPN")
                   .addAddress("10.0.0.2", 32)
                   .addDnsServer("8.8.8.8")
                   .addRoute("0.0.0.0", 0)
                   .setMtu(1500);

            vpnInterface = builder.establish();
            if (vpnInterface != null) {
                int fd = vpnInterface.getFd();
                startTorWithTun(fd);  // Call Rust JNI
            }
        } catch (Exception e) {
            Log.e(TAG, "Failed to start VPN", e);
        }
        return START_STICKY;
    }

    private native void startTorWithTun(int tunFd);

    @Override
    public void onDestroy() {
        super.onDestroy();
        if (vpnInterface != null) {
            try { vpnInterface.close(); } catch (Exception ignored) {}
        }
        stopTorNative();
    }

    private native void stopTorNative();
}
