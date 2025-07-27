use specta_typescript::Typescript;
use tauri_specta::{collect_commands, Builder};

#[tauri::command]
#[specta::specta] // < You must annotate your commands
fn hello_world(my_name: String) -> String {
    format!("Hello, {my_name}! You've been greeted from Rust!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new()
        // Then register them (separated by a comma)
        .commands(collect_commands![hello_world,]);

    #[cfg(debug_assertions)] // <- Only export on non-release builds
    builder
        .export(Typescript::default(), "../src/bindings.ts")
        .expect("Failed to export typescript bindings");

    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
