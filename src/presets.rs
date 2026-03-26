use crate::lib::SniConfig;

pub fn get_sni_presets() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Cloudflare", "www.cloudflare.com"),
        ("Microsoft", "www.microsoft.com"),
        ("Google", "www.google.com"),
        ("VK.ru (good for RU)", "vk.ru"),
        ("Yandex", "ya.ru"),
        ("Apple", "www.apple.com"),
        ("Amazon", "www.amazon.com"),
        ("GitHub", "github.com"),
    ]
}

pub fn get_bridge_presets() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // WebTunnel with real fingerprint (example – replace with actual ones)
        ("WebTunnel + Cloudflare SNI", 
         "www.cloudflare.com", 
         "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=0xD99B8A5B3F7E2A6C"),
        
        ("WebTunnel + Microsoft SNI", 
         "www.microsoft.com", 
         "webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com fingerprint=0xABCDEF1234567890"),
        
        ("WebTunnel + VK.ru SNI", 
         "vk.ru", 
         "webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru fingerprint=0x1234567890ABCDEF"),
        
        // Obfs4 bridge (example – replace with actual bridge from BridgeDB)
        ("Obfs4 (default Tor)", 
         "", 
         "obfs4 5.230.119.38:22333 8B920DA77C4078FBCF0491BB39B3B974EA973ACF cert=I3LUTdY2yJkwcORkM+8vV1iGcNc5tA9w+7Fj6Y0= iat-mode=0"),
    ]
}
