#![allow(non_snake_case)]

use slint::SharedString;
use aix::{SniConfig, TOR_MANAGER};
use android_activity::AndroidApp;

slint::include_modules!();

fn main() {
    // Initialize logging for Android
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );

    let app = App::new().unwrap();

    let app_weak = app.as_weak();
    app.on_connect(move |sni, bridge| {
        let app = app_weak.unwrap();
        let sni_str: String = sni.into();
        let bridge_str: String = bridge.into();

        // Update log immediately
        app.set_log(SharedString::from("Updating SNI and starting Tor..."));

        let cfg = SniConfig {
            enabled: true,
            custom_sni: sni_str.clone(),
            bridge_line: bridge_str.clone(),
            last_updated: None,
        };
        let tm = TOR_MANAGER.clone();

        // Spawn async task to start Tor
        tokio::spawn(async move {
            if let Err(e) = tm.update_sni(cfg).await {
                app.set_log(SharedString::from(format!("SNI error: {}", e)));
                return;
            }
            match tm.start_tor().await {
                Ok(msg) => {
                    app.set_status(SharedString::from(&msg));
                    app.set_log(SharedString::from("✅ Tor + Custom SNI started!"));
                }
                Err(e) => {
                    app.set_status(SharedString::from(format!("❌ Failed: {}", e)));
                    app.set_log(SharedString::from(format!("Error: {}", e)));
                }
            }
        });
    });

    let app_weak = app.as_weak();
    app.on_disconnect(move || {
        let app = app_weak.unwrap();
        let tm = TOR_MANAGER.clone();
        tokio::spawn(async move {
            tm.stop_tor().await;
            app.set_status(SharedString::from("🔴 Disconnected"));
            app.set_log(SharedString::from("Tor stopped."));
        });
    });

    // Run the Slint event loop
    app.run();
}
