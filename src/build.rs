use std::fs;
use std::path::Path;

fn main() {
    let out_dir = "assets";
    let file_path = format!("{}/htmx.min.js", out_dir);
    // Используйте конкретную версию для стабильности
    let url = "https://cdn.jsdelivr.net/npm/htmx.org@2.0.10/dist/htmx.min.js";

    // Создаем директорию, если пользователь только что склонировал проект
    if !Path::new(out_dir).exists() {
        fs::create_dir_all(out_dir).expect("Failed to create assets directory");
    }

    if !Path::new(&file_path).exists() {
        println!("cargo:warning=HTMX not found. Downloading from CDN...");
        
        let mut response = reqwest::blocking::get(url)
            .expect("Failed to connect to CDN");

        if response.status().is_success() {
            let mut file = fs::File::create(&file_path).expect("Failed to create file");
            response.copy_to(&mut file).expect("Failed to save HTMX content");
            println!("cargo:warning=HTMX downloaded successfully.");
        } else {
            panic!("Failed to download HTMX: Status {}", response.status());
        }
    }

    // Сообщаем Cargo, что если файл все же появится/изменится, нужно учитывать это
    println!("cargo:rerun-if-changed=build.rs");
}
