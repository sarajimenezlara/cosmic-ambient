//! cosmic-ambient: fondos de escritorio para COSMIC, cada uno con su ambiente
//! (colores del sistema, reloj y visualizador de música).
//!
//! Sin argumentos abre la ventana. Comandos:
//!   cosmic-ambient añadir <imagen> <nombre>   mete una imagen en el almacén
//!   cosmic-ambient lista                      lista los fondos (* = el puesto)
//!   cosmic-ambient aplicar <nombre>           pone un fondo con su configuración
//!   cosmic-ambient quitar <nombre>            lo saca del almacén
//!   cosmic-ambient perfil <nombre>            ruta de su perfil.toml
//!   cosmic-ambient cambiar-imagen <nombre> <imagen>
//!                                             otra imagen, misma configuración
//! (también en inglés: add, list, apply, remove, profile, set-image)

mod gui;
mod store;
mod tema;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let resultado = match args.as_slice() {
        [] => {
            gui::run();
            Ok(())
        }
        ["añadir" | "anadir" | "add", imagen, nombre @ ..] if !nombre.is_empty() => {
            store::anadir(Path::new(imagen), &nombre.join(" "))
                .map(|f| println!("Añadido «{}» ({})", f.perfil.nombre, f.dir.display()))
                .map_err(|e| e.to_string())
        }
        ["lista" | "list"] => {
            let activo = store::activo();
            for f in store::listar() {
                let marca = if activo.as_deref() == Some(f.id.as_str()) { "*" } else { " " };
                println!("{marca} {:<24} {}", f.id, f.perfil.nombre);
            }
            Ok(())
        }
        ["aplicar" | "apply", nombre @ ..] if !nombre.is_empty() => con_fondo(nombre, |f| {
            let avisos = store::aplicar(&f);
            for a in &avisos {
                eprintln!("aviso: {a}");
            }
            println!("Puesto «{}»", f.perfil.nombre);
            Ok(())
        }),
        ["quitar" | "remove", nombre @ ..] if !nombre.is_empty() => con_fondo(nombre, |f| {
            store::quitar(&f).map_err(|e| e.to_string())?;
            println!("Quitado «{}»", f.perfil.nombre);
            Ok(())
        }),
        ["cambiar-imagen" | "set-image", nombre, imagen] => con_fondo(&[nombre], |mut f| {
            store::cambiar_imagen(&mut f, Path::new(imagen)).map_err(|e| e.to_string())?;
            println!("Cambiada la imagen de «{}»", f.perfil.nombre);
            if store::activo().as_deref() == Some(f.id.as_str()) {
                for a in store::aplicar(&f) {
                    eprintln!("aviso: {a}");
                }
            }
            Ok(())
        }),
        ["perfil" | "profile", nombre @ ..] if !nombre.is_empty() => con_fondo(nombre, |f| {
            println!("{}", f.dir.join("perfil.toml").display());
            Ok(())
        }),
        _ => Err("uso: cosmic-ambient [añadir <imagen> <nombre> | lista | aplicar <nombre> | quitar <nombre> | perfil <nombre> | cambiar-imagen <nombre> <imagen>]".into()),
    };

    match resultado {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn con_fondo(nombre: &[&str], f: impl FnOnce(store::Fondo) -> Result<(), String>) -> Result<(), String> {
    let nombre = nombre.join(" ");
    match store::buscar(&nombre) {
        Some(fondo) => f(fondo),
        None => Err(format!("no hay ningún fondo llamado «{nombre}» (mira «cosmic-ambient lista»)")),
    }
}
