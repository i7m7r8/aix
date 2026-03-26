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
        // WebTunnel with SNI imitation (modern & effective)
        ("WebTunnel + Cloudflare SNI", 
         "www.cloudflare.com", 
         "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=..."),
        
        ("WebTunnel + Microsoft SNI", 
         "www.microsoft.com", 
         "webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com fingerprint=..."),
        
        ("WebTunnel + VK.ru SNI", 
         "vk.ru", 
         "webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru fingerprint=..."),
        
        ("Meek-like (Azure)", 
         "ajax.aspnetcdn.com", 
         "meek_lite 192.0.2.1:443 url=https://ajax.aspnetcdn.com/ fingerprint=..."),
        
        ("Default obfs4 (no SNI)", 
         "", 
         "obfs4 5.230.119.38:22333 8B920DA77C4078FBCF0491BB39B3B974EA973ACF cert=... iat-mode=0"),
    ]
}
