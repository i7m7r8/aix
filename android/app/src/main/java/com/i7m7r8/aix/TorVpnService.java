package com.i7m7r8.aix;

import android.net.VpnService;
import android.os.ParcelFileDescriptor;
import android.util.Log;
import android.system.OsConstants;
import android.content.Intent;
import androidx.localbroadcastmanager.content.LocalBroadcastManager;
import java.io.*;

public class TorVpnService extends VpnService {
    private static final String TAG = "AIX_VPN";
    private ParcelFileDescriptor vpnInterface = null;
    private Thread torThread;

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        Log.i(TAG, "Starting AIX Tor VPN");
        
        String sni = intent.getStringExtra("sni");
        String bridge = intent.getStringExtra("bridge");
        
        if (sni == null || bridge == null) {
            // Try to read from saved config
            try {
                File configFile = new File(getFilesDir(), "current_config.json");
                if (configFile.exists()) {
                    StringBuilder json = new StringBuilder();
                    try (BufferedReader reader = new BufferedReader(new InputStreamReader(new FileInputStream(configFile)))) {
                        String line;
                        while ((line = reader.readLine()) != null) {
                            json.append(line);
                        }
                    }
                    org.json.JSONObject obj = new org.json.JSONObject(json.toString());
                    sni = obj.getString("custom_sni");
                    bridge = obj.getString("bridge_line");
                    Log.i(TAG, "Loaded config from file: SNI=" + sni);
                }
            } catch (Exception e) {
                Log.e(TAG, "Failed to read config", e);
            }
        }
        
        if (sni == null || bridge == null) {
            Log.e(TAG, "No SNI/bridge configuration found");
            stopSelf();
            return START_NOT_STICKY;
        }
        
        try {
            Builder builder = new Builder();
            builder.setSession("AIX Tor VPN with Custom SNI")
                   .addAddress("10.0.0.2", 32)
                   .addDnsServer("8.8.8.8")
                   .addRoute("0.0.0.0", 0)
                   .addRoute("::", 0)
                   .setMtu(1500)
                   .allowFamily(OsConstants.AF_INET)
                   .allowFamily(OsConstants.AF_INET6);
            
            vpnInterface = builder.establish();
            if (vpnInterface != null) {
                int fd = vpnInterface.getFd();
                broadcastStatus("🟢 Connecting...", "Starting Tor with SNI: " + sni, false);
                startTorWithTun(fd, sni, bridge);
            }
        } catch (Exception e) {
            Log.e(TAG, "VPN start failed", e);
            broadcastStatus("❌ Failed: " + e.getMessage(), "Error: " + e.getMessage(), false);
            stopSelf();
        }
        return START_STICKY;
    }
    
    private void broadcastStatus(String status, String log, boolean connected) {
        Intent intent = new Intent("AIX_VPN_STATUS");
        intent.putExtra("status", status);
        intent.putExtra("log", log);
        intent.putExtra("connected", connected);
        LocalBroadcastManager.getInstance(this).sendBroadcast(intent);
    }
    
    private native void startTorWithTun(int tunFd, String sni, String bridge);
    private native void stopTorNative();
    
    @Override
    public void onDestroy() {
        broadcastStatus("🔴 Disconnected", "Tor stopped.", false);
        stopTorNative();
        if (vpnInterface != null) {
            try { vpnInterface.close(); } catch (Exception ignored) {}
        }
        super.onDestroy();
    }
}
