package com.i7m7r8.aix;

import android.Manifest;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageManager;
import android.net.VpnService;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.View;
import android.widget.*;
import androidx.annotation.NonNull;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.app.ActivityCompat;
import androidx.core.content.ContextCompat;
import androidx.localbroadcastmanager.content.LocalBroadcastManager;
import com.google.android.material.snackbar.Snackbar;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.*;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

public class MainActivity extends AppCompatActivity {
    private static final int VPN_REQUEST_CODE = 1001;
    private static final int STORAGE_PERMISSION_CODE = 1002;
    
    private EditText sniInput;
    private EditText bridgeInput;
    private TextView statusText;
    private TextView logText;
    private Button connectButton;
    private Button disconnectButton;
    private Button savePresetButton;
    private ListView presetsListView;
    private ProgressBar progressBar;
    private Switch alwaysOnSwitch;
    
    private ArrayAdapter<String> presetsAdapter;
    private List<String> presetNames = new ArrayList<>();
    private List<String> presetSnips = new ArrayList<>();
    private List<String> presetBridges = new ArrayList<>();
    
    private boolean isConnected = false;
    private Handler uiHandler = new Handler(Looper.getMainLooper());
    
    // Broadcast receiver for VPN service status updates
    private BroadcastReceiver vpnStatusReceiver = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            String status = intent.getStringExtra("status");
            String log = intent.getStringExtra("log");
            boolean connected = intent.getBooleanExtra("connected", false);
            
            if (status != null) updateStatus(status);
            if (log != null) appendLog(log);
            if (connected) {
                isConnected = true;
                updateUIForConnection(true);
            }
        }
    };
    
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        
        // Initialize views
        sniInput = findViewById(R.id.sni_input);
        bridgeInput = findViewById(R.id.bridge_input);
        statusText = findViewById(R.id.status_text);
        logText = findViewById(R.id.log_text);
        connectButton = findViewById(R.id.connect_button);
        disconnectButton = findViewById(R.id.disconnect_button);
        savePresetButton = findViewById(R.id.save_preset_button);
        presetsListView = findViewById(R.id.presets_list);
        progressBar = findViewById(R.id.progress_bar);
        alwaysOnSwitch = findViewById(R.id.always_on_switch);
        
        // Load saved presets
        loadPresets();
        
        // Setup presets list adapter
        presetsAdapter = new ArrayAdapter<>(this, android.R.layout.simple_list_item_1, presetNames);
        presetsListView.setAdapter(presetsAdapter);
        
        // Load saved settings
        loadSettings();
        
        // Setup click listeners
        connectButton.setOnClickListener(v -> startVpnService());
        disconnectButton.setOnClickListener(v -> stopVpnService());
        savePresetButton.setOnClickListener(v -> saveCurrentPreset());
        
        presetsListView.setOnItemClickListener((parent, view, position, id) -> {
            applyPreset(position);
        });
        
        presetsListView.setOnItemLongClickListener((parent, view, position, id) -> {
            deletePreset(position);
            return true;
        });
        
        alwaysOnSwitch.setOnCheckedChangeListener((buttonView, isChecked) -> {
            saveAlwaysOnSetting(isChecked);
        });
        
        // Register broadcast receiver
        LocalBroadcastManager.getInstance(this).registerReceiver(
            vpnStatusReceiver,
            new IntentFilter("AIX_VPN_STATUS")
        );
        
        // Check VPN permission
        checkVpnPermission();
    }
    
    private void checkVpnPermission() {
        if (VpnService.prepare(this) != null) {
            Intent intent = VpnService.prepare(this);
            startActivityForResult(intent, VPN_REQUEST_CODE);
        } else {
            // Already prepared
            appendLog("VPN permission granted");
        }
    }
    
    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == VPN_REQUEST_CODE) {
            if (resultCode == RESULT_OK) {
                appendLog("VPN permission granted");
            } else {
                appendLog("VPN permission denied");
                Snackbar.make(findViewById(android.R.id.content), 
                    "VPN permission required for app to work", Snackbar.LENGTH_LONG).show();
            }
        }
    }
    
    private void startVpnService() {
        String sni = sniInput.getText().toString().trim();
        String bridge = bridgeInput.getText().toString().trim();
        
        if (sni.isEmpty()) {
            sniInput.setError("SNI required");
            return;
        }
        
        if (bridge.isEmpty()) {
            bridgeInput.setError("Bridge line required");
            return;
        }
        
        appendLog("Starting VPN service...");
        showProgress(true);
        
        // Save current config
        saveCurrentConfig(sni, bridge);
        
        Intent intent = new Intent(this, TorVpnService.class);
        intent.putExtra("sni", sni);
        intent.putExtra("bridge", bridge);
        
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent);
        } else {
            startService(intent);
        }
    }
    
    private void stopVpnService() {
        appendLog("Stopping VPN service...");
        Intent intent = new Intent(this, TorVpnService.class);
        stopService(intent);
        updateStatus("🔴 Stopping...");
        uiHandler.postDelayed(() -> {
            updateStatus("🔴 Disconnected");
            isConnected = false;
            updateUIForConnection(false);
            showProgress(false);
        }, 1000);
    }
    
    private void updateStatus(String status) {
        runOnUiThread(() -> {
            statusText.setText(status);
            if (status.contains("Connected") || status.contains("✅")) {
                statusText.setTextColor(getColor(android.R.color.holo_green_dark));
            } else if (status.contains("Disconnected") || status.contains("🔴")) {
                statusText.setTextColor(getColor(android.R.color.holo_red_dark));
            } else {
                statusText.setTextColor(getColor(android.R.color.holo_orange_dark));
            }
        });
    }
    
    private void appendLog(String message) {
        runOnUiThread(() -> {
            String currentLog = logText.getText().toString();
            String timestamp = new java.text.SimpleDateFormat("HH:mm:ss", java.util.Locale.getDefault())
                .format(new java.util.Date());
            String newLog = currentLog + "\n[" + timestamp + "] " + message;
            // Keep last 50 lines
            String[] lines = newLog.split("\n");
            if (lines.length > 50) {
                newLog = String.join("\n", java.util.Arrays.copyOfRange(lines, lines.length - 50, lines.length));
            }
            logText.setText(newLog);
            // Auto-scroll to bottom
            final TextView textView = logText;
            textView.post(() -> {
                int scrollAmount = textView.getLayout().getLineTop(textView.getLineCount()) - textView.getHeight();
                if (scrollAmount > 0)
                    textView.scrollTo(0, scrollAmount);
                else
                    textView.scrollTo(0, 0);
            });
        });
    }
    
    private void showProgress(boolean show) {
        runOnUiThread(() -> {
            if (show) {
                progressBar.setVisibility(View.VISIBLE);
                connectButton.setEnabled(false);
            } else {
                progressBar.setVisibility(View.GONE);
                connectButton.setEnabled(!isConnected);
            }
        });
    }
    
    private void updateUIForConnection(boolean connected) {
        runOnUiThread(() -> {
            if (connected) {
                connectButton.setEnabled(false);
                disconnectButton.setEnabled(true);
                sniInput.setEnabled(false);
                bridgeInput.setEnabled(false);
            } else {
                connectButton.setEnabled(true);
                disconnectButton.setEnabled(false);
                sniInput.setEnabled(true);
                bridgeInput.setEnabled(true);
            }
        });
    }
    
    private void saveCurrentPreset() {
        String name = sniInput.getText().toString().trim();
        String sni = sniInput.getText().toString().trim();
        String bridge = bridgeInput.getText().toString().trim();
        
        if (name.isEmpty() || sni.isEmpty()) {
            Snackbar.make(findViewById(android.R.id.content), 
                "Enter a name and SNI first", Snackbar.LENGTH_SHORT).show();
            return;
        }
        
        // Check if preset exists
        int index = presetNames.indexOf(name);
        if (index >= 0) {
            presetSnips.set(index, sni);
            presetBridges.set(index, bridge);
        } else {
            presetNames.add(name);
            presetSnips.add(sni);
            presetBridges.add(bridge);
        }
        
        savePresets();
        presetsAdapter.notifyDataSetChanged();
        appendLog("Saved preset: " + name);
    }
    
    private void applyPreset(int position) {
        String sni = presetSnips.get(position);
        String bridge = presetBridges.get(position);
        sniInput.setText(sni);
        bridgeInput.setText(bridge);
        appendLog("Loaded preset: " + presetNames.get(position));
    }
    
    private void deletePreset(int position) {
        String name = presetNames.get(position);
        presetNames.remove(position);
        presetSnips.remove(position);
        presetBridges.remove(position);
        savePresets();
        presetsAdapter.notifyDataSetChanged();
        appendLog("Deleted preset: " + name);
    }
    
    private void savePresets() {
        try {
            JSONArray jsonArray = new JSONArray();
            for (int i = 0; i < presetNames.size(); i++) {
                JSONObject obj = new JSONObject();
                obj.put("name", presetNames.get(i));
                obj.put("sni", presetSnips.get(i));
                obj.put("bridge", presetBridges.get(i));
                jsonArray.put(obj);
            }
            String data = jsonArray.toString();
            try (FileOutputStream fos = openFileOutput("presets.json", MODE_PRIVATE)) {
                fos.write(data.getBytes());
            }
        } catch (Exception e) {
            appendLog("Error saving presets: " + e.getMessage());
        }
    }
    
    private void loadPresets() {
        try {
            File file = new File(getFilesDir(), "presets.json");
            if (file.exists()) {
                byte[] data = new byte[(int) file.length()];
                try (FileInputStream fis = new FileInputStream(file)) {
                    fis.read(data);
                }
                String json = new String(data);
                JSONArray jsonArray = new JSONArray(json);
                for (int i = 0; i < jsonArray.length(); i++) {
                    JSONObject obj = jsonArray.getJSONObject(i);
                    presetNames.add(obj.getString("name"));
                    presetSnips.add(obj.getString("sni"));
                    presetBridges.add(obj.getString("bridge"));
                }
                appendLog("Loaded " + presetNames.size() + " presets");
            }
        } catch (Exception e) {
            appendLog("Error loading presets: " + e.getMessage());
        }
        
        // Add default presets if none exist
        if (presetNames.isEmpty()) {
            addDefaultPresets();
        }
    }
    
    private void addDefaultPresets() {
        presetNames.add("Cloudflare");
        presetSnips.add("www.cloudflare.com");
        presetBridges.add("webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=0xD99B8A5B3F7E2A6C");
        
        presetNames.add("VK.ru");
        presetSnips.add("vk.ru");
        presetBridges.add("webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru fingerprint=0x1234567890ABCDEF");
        
        presetNames.add("Microsoft");
        presetSnips.add("www.microsoft.com");
        presetBridges.add("webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com fingerprint=0xABCDEF1234567890");
        
        presetNames.add("Yandex");
        presetSnips.add("ya.ru");
        presetBridges.add("webtunnel 185.220.101.3:443 sni-imitation=ya.ru fingerprint=0x9876543210FEDCBA");
        
        savePresets();
    }
    
    private void saveCurrentConfig(String sni, String bridge) {
        try {
            JSONObject config = new JSONObject();
            config.put("custom_sni", sni);
            config.put("bridge_line", bridge);
            config.put("enabled", true);
            config.put("last_updated", System.currentTimeMillis());
            
            try (FileOutputStream fos = openFileOutput("current_config.json", MODE_PRIVATE)) {
                fos.write(config.toString().getBytes());
            }
        } catch (Exception e) {
            appendLog("Error saving config: " + e.getMessage());
        }
    }
    
    private void loadSettings() {
        try {
            File file = new File(getFilesDir(), "current_config.json");
            if (file.exists()) {
                byte[] data = new byte[(int) file.length()];
                try (FileInputStream fis = new FileInputStream(file)) {
                    fis.read(data);
                }
                String json = new String(data);
                JSONObject obj = new JSONObject(json);
                sniInput.setText(obj.optString("custom_sni", "www.cloudflare.com"));
                bridgeInput.setText(obj.optString("bridge_line", ""));
            }
        } catch (Exception e) {
            appendLog("Error loading settings: " + e.getMessage());
        }
        
        // Load always-on setting
        boolean alwaysOn = getSharedPreferences("aix_prefs", MODE_PRIVATE)
            .getBoolean("always_on", false);
        alwaysOnSwitch.setChecked(alwaysOn);
    }
    
    private void saveAlwaysOnSetting(boolean enabled) {
        getSharedPreferences("aix_prefs", MODE_PRIVATE)
            .edit()
            .putBoolean("always_on", enabled)
            .apply();
        
        if (enabled && isConnected) {
            // TODO: Implement always-on VPN logic
            appendLog("Always-on VPN enabled - will auto-reconnect");
        }
    }
    
    @Override
    protected void onDestroy() {
        super.onDestroy();
        LocalBroadcastManager.getInstance(this).unregisterReceiver(vpnStatusReceiver);
    }
}
