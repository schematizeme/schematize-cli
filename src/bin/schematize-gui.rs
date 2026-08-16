//! schematize-gui — atalho gráfico. É o MESMO software do `schematize`: só abre a janela.
//! Equivale a `schematize gui`. Mantido pro lançador de desktop e por compat.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    schematize::gui::run()
}
