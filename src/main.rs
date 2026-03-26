#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_mobile::launch;
use crate::lib::{TorManager, SniConfig};
use std::sync::Arc;
use once_cell::sync::Lazy;

static TOR_MANAGER: Lazy<Arc<TorManager>> = Lazy::new(|| Arc::new(TorManager::new()));

fn App() -> Element {
    let mut status = use_signal(|| "🔴 Disconnected".to_string());
    let mut sni_input = use_signal(|| "www.cloudflare.com".to_string());
    let mut bridge_input = use_signal(|| "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string());
    let mut is_connected = use_signal(|| false);
    let mut log = use_signal(|| "Edit Custom SNI below and tap CONNECT".to_string());

    let connect = move |_| {
        let tm = TOR_MANAGER.clone();
        let cfg = SniConfig {
            enabled: true,
            custom_sni: sni_input.read().clone(),
            bridge_line: bridge_input.read().clone(),
            last_updated: None,
        };

        spawn(async move {
            if let Err(e) = tm.update_sni(cfg).await {
                log.set(format!("SNI update error: {}", e));
                return;
            }
            match tm.start_tor().await {
                Ok(msg) => {
                    status.set(msg);
                    is_connected.set(true);
                    log.set("Tor bootstrapped successfully with your custom SNI!".to_string());
                }
                Err(e) => {
                    status.set(format!("❌ {}", e));
                    log.set(format!("Error: {}", e));
                }
            }
        });
    };

    let disconnect = move |_| {
        let tm = TOR_MANAGER.clone();
        spawn(async move {
            tm.stop_tor().await;
            status.set("🔴 Disconnected".to_string());
            is_connected.set(false);
            log.set("Tor stopped.".to_string());
        });
    };

    rsx! {
        div { class: "min-h-screen bg-zinc-950 text-white flex flex-col",
            // Premium Header
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
                // Custom SNI - Main Feature
                div { class: "bg-zinc-900 rounded-3xl p-8 border border-zinc-700",
                    h2 { class: "text-2xl font-semibold mb-6 flex items-center gap-3", "🎯 Custom SNI" }
                    input {
                        class: "w-full bg-zinc-800 border border-zinc-700 rounded-2xl px-6 py-5 text-lg focus:border-violet-500 focus:outline-none",
                        placeholder: "www.microsoft.com or cloudflare.com",
                        value: "{sni_input}",
                        oninput: move |e| sni_input.set(e.value())
                    }
                    p { class: "text-xs text-zinc-500 mt-4", "Used for SNI imitation in pluggable transports (webtunnel, meek, etc.)" }
                }

                // Bridge Line
                div { class: "bg-zinc-900 rounded-3xl p-8 border border-zinc-700",
                    h2 { class: "text-xl font-semibold mb-4", "🌉 Full Bridge Line" }
                    textarea {
                        class: "w-full h-36 bg-zinc-950 border border-zinc-700 rounded-2xl p-6 font-mono text-sm",
                        value: "{bridge_input}",
                        oninput: move |e| bridge_input.set(e.value())
                    }
                }

                // Live Log
                div { class: "bg-black/70 rounded-3xl p-6 h-44 overflow-auto font-mono text-sm text-emerald-300",
                    "{log}"
                }
            }

            // Big Action Buttons
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

            // Bottom Navigation
            nav { class: "bg-zinc-900 border-t border-zinc-800 py-5 flex justify-around text-xs",
                div { class: "text-violet-400", "🏠 Home" }
                div { "🌉 Bridges" }
                div { "⚙️ Settings" }
            }
        }
    }
}

fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    launch(App);
}
