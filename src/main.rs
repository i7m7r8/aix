#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_mobile::{launch, use_android_context};
use aix::{SniConfig, TOR_MANAGER};
use std::sync::Arc;

mod presets;

fn App() -> Element {
    let mut status = use_signal(|| "🔴 Disconnected".to_string());
    let mut sni_input = use_signal(|| "www.cloudflare.com".to_string());
    let mut bridge_input = use_signal(|| "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string());
    let mut is_connected = use_signal(|| false);
    let mut log = use_signal(|| "Edit Custom SNI or use presets below → then tap CONNECT".to_string());

    let android_context = use_android_context();

    let connect = move |_| {
        let sni = sni_input.read().clone();
        let bridge = bridge_input.read().clone();
        let context = android_context.clone();

        spawn(async move {
            log.set("Updating SNI and starting Tor...".to_string());

            let cfg = SniConfig {
                enabled: true,
                custom_sni: sni,
                bridge_line: bridge,
                last_updated: None,
            };

            if let Err(e) = TOR_MANAGER.update_sni(cfg).await {
                log.set(format!("SNI error: {}", e));
                return;
            }

            match TOR_MANAGER.start_tor().await {
                Ok(msg) => {
                    status.set(msg);
                    is_connected.set(true);
                    log.set("✅ Tor + Custom SNI started!".to_string());
                }
                Err(e) => {
                    status.set(format!("❌ Failed: {}", e));
                    log.set(format!("Error: {}", e));
                }
            }
        });
    };

    let disconnect = move |_| {
        spawn(async move {
            TOR_MANAGER.stop_tor().await;
            status.set("🔴 Disconnected".to_string());
            is_connected.set(false);
            log.set("Tor stopped.".to_string());
        });
    };

    rsx! {
        div { class: "min-h-screen bg-zinc-950 text-white flex flex-col",
            header { class: "bg-gradient-to-br from-violet-600 to-fuchsia-600 p-8",
                div { class: "flex justify-between items-start",
                    div {
                        h1 { class: "text-5xl font-black tracking-tighter", "AIX" }
                        p { class: "text-violet-200 mt-1 text-lg", "Pure Rust Tor • Custom SNI" }
                    }
                    div { class: if *is_connected { "text-emerald-400 font-medium" } else { "text-red-400 font-medium" },
                        "{status}"
                    }
                }
            }

            main { class: "flex-1 p-6 space-y-8",
                div { class: "bg-zinc-900 rounded-3xl p-8 border border-zinc-700",
                    h2 { class: "text-2xl font-semibold mb-6 flex items-center gap-3", "🎯 Custom SNI" }
                    input {
                        class: "w-full bg-zinc-800 border border-zinc-700 rounded-2xl px-6 py-5 text-lg focus:border-violet-500 focus:outline-none",
                        placeholder: "www.example.com",
                        value: "{sni_input}",
                        oninput: move |e| sni_input.set(e.value())
                    }
                    p { class: "text-xs text-zinc-500 mt-4", "Used for SNI imitation in pluggable transports (webtunnel, meek, etc.)" }
                }

                div { class: "bg-zinc-900 rounded-3xl p-8 border border-zinc-700 mt-6",
                    h2 { class: "text-xl font-semibold mb-5 flex items-center gap-2", "⚡ Quick Presets" }
                    div { class: "grid grid-cols-2 gap-3 text-sm",
                        button { class: "bg-zinc-800 hover:bg-violet-600/30 py-3 px-4 rounded-2xl transition-colors active:scale-95", onclick: move |_| { sni_input.set("www.cloudflare.com".to_string()); bridge_input.set("webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string()); log.set("✅ Loaded Cloudflare SNI preset".to_string()); }, "Cloudflare SNI" }
                        button { class: "bg-zinc-800 hover:bg-violet-600/30 py-3 px-4 rounded-2xl transition-colors active:scale-95", onclick: move |_| { sni_input.set("vk.ru".to_string()); bridge_input.set("webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru fingerprint=...".to_string()); log.set("✅ Loaded VK.ru SNI".to_string()); }, "VK.ru SNI" }
                        button { class: "bg-zinc-800 hover:bg-violet-600/30 py-3 px-4 rounded-2xl transition-colors active:scale-95", onclick: move |_| { sni_input.set("www.microsoft.com".to_string()); bridge_input.set("webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com fingerprint=...".to_string()); log.set("✅ Loaded Microsoft SNI".to_string()); }, "Microsoft SNI" }
                        button { class: "bg-zinc-800 hover:bg-violet-600/30 py-3 px-4 rounded-2xl transition-colors active:scale-95", onclick: move |_| { sni_input.set("ya.ru".to_string()); bridge_input.set("webtunnel 185.220.101.3:443 sni-imitation=ya.ru fingerprint=...".to_string()); log.set("✅ Loaded Yandex SNI".to_string()); }, "Yandex SNI" }
                    }
                    p { class: "text-xs text-zinc-500 mt-5", "Tap preset → edit SNI freely → then CONNECT" }
                }

                div { class: "bg-zinc-900 rounded-3xl p-8 border border-zinc-700",
                    h2 { class: "text-xl font-semibold mb-4", "🌉 Bridge Line" }
                    textarea {
                        class: "w-full h-36 bg-zinc-950 border border-zinc-700 rounded-2xl p-6 font-mono text-sm",
                        value: "{bridge_input}",
                        oninput: move |e| bridge_input.set(e.value())
                    }
                }

                div { class: "bg-black/70 rounded-3xl p-6 h-44 overflow-auto font-mono text-sm text-emerald-300", "{log}" }
            }

            div { class: "p-6 grid grid-cols-2 gap-4 pb-8",
                button {
                    class: "py-7 rounded-3xl bg-emerald-600 font-bold text-xl active:bg-emerald-500 shadow-lg",
                    onclick: connect,
                    "CONNECT"
                }
                button {
                    class: "py-7 rounded-3xl bg-red-600 font-bold text-xl active:bg-red-500 shadow-lg",
                    onclick: disconnect,
                    "DISCONNECT"
                }
            }

            nav { class: "bg-zinc-900 border-t border-zinc-800 py-5 flex justify-around text-xs",
                div { class: "text-violet-400", "🏠 Home" }
                div { "🌉 Bridges" }
                div { "⚙️ Settings" }
            }
        }
    }
}

fn main() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );
    launch(App);
}
