pub type Matrix = [[f32; 5]; 5];

pub fn matrix_from_transform(transform: [f32; 25]) -> Matrix {
    std::array::from_fn(|row| std::array::from_fn(|column| transform[row * 5 + column]))
}
pub fn transform_from_matrix(matrix: Matrix) -> [f32; 25] {
    std::array::from_fn(|index| matrix[index / 5][index % 5])
}
pub fn matrices_match(left: Matrix, right: Matrix) -> bool {
    left.iter()
        .flatten()
        .zip(right.iter().flatten())
        .all(|(a, b)| a.is_finite() && b.is_finite() && (a - b).abs() <= 0.00001)
}

/// The production transaction and the tests use exactly the same lifecycle.
pub trait MagnificationApi: Send {
    fn initialize(&mut self) -> Result<(), String>;
    fn read(&mut self) -> Result<Matrix, String>;
    fn write(&mut self, matrix: Matrix) -> Result<(), String>;
    fn uninitialize(&mut self) -> Result<(), String>;
}
pub struct NativeMagnification;

#[cfg(target_os = "windows")]
impl MagnificationApi for NativeMagnification {
    fn initialize(&mut self) -> Result<(), String> {
        if unsafe { windows::Win32::UI::Magnification::MagInitialize().as_bool() } {
            Ok(())
        } else {
            Err("MagInitialize devolvió FALSE; Windows rechazó la inicialización.".into())
        }
    }
    fn read(&mut self) -> Result<Matrix, String> {
        use windows::Win32::UI::Magnification::{MAGCOLOREFFECT, MagGetFullscreenColorEffect};
        let mut effect = MAGCOLOREFFECT {
            transform: [0.0; 25],
        };
        if unsafe { MagGetFullscreenColorEffect(&mut effect).as_bool() } {
            let matrix = matrix_from_transform(effect.transform);
            if matrix.iter().flatten().all(|v| v.is_finite()) {
                Ok(matrix)
            } else {
                Err("Windows devolvió una matriz con valores no finitos.".into())
            }
        } else {
            Err("MagGetFullscreenColorEffect devolvió FALSE; no se pudo leer el efecto.".into())
        }
    }
    fn write(&mut self, matrix: Matrix) -> Result<(), String> {
        use windows::Win32::UI::Magnification::{MAGCOLOREFFECT, MagSetFullscreenColorEffect};
        let effect = MAGCOLOREFFECT {
            transform: transform_from_matrix(matrix),
        };
        if unsafe { MagSetFullscreenColorEffect(&effect).as_bool() } {
            Ok(())
        }
        // This API documents BOOL, not a reliable GetLastError contract.
        else {
            Err("MagSetFullscreenColorEffect devolvió FALSE; Windows rechazó el efecto.".into())
        }
    }
    fn uninitialize(&mut self) -> Result<(), String> {
        if unsafe { windows::Win32::UI::Magnification::MagUninitialize().as_bool() } {
            Ok(())
        } else {
            Err("MagUninitialize devolvió FALSE; liberación pendiente.".into())
        }
    }
}
#[cfg(not(target_os = "windows"))]
impl MagnificationApi for NativeMagnification {
    fn initialize(&mut self) -> Result<(), String> {
        Err("Solo disponible en Windows.".into())
    }
    fn read(&mut self) -> Result<Matrix, String> {
        Err("Solo disponible en Windows.".into())
    }
    fn write(&mut self, _: Matrix) -> Result<(), String> {
        Err("Solo disponible en Windows.".into())
    }
    fn uninitialize(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Shared restoration policy for the legacy engine and the independent watchdog.
pub fn restore_known<A: MagnificationApi>(
    api: &mut A,
    original: Matrix,
    known: &[Matrix],
    accepted: &mut Option<bool>,
) -> Result<(), String> {
    let current = api.read()?;
    if !matrices_match(current, original)
        && !known.iter().any(|matrix| matrices_match(current, *matrix))
    {
        return Err("Conflicto: otra aplicación cambió la matriz global. Diario conservado; no se sobrescribió su efecto.".into());
    }
    if !matrices_match(current, original) {
        *accepted = Some(false);
        api.write(original)?;
        *accepted = Some(true);
        if !matrices_match(api.read()?, original) {
            return Err(
                "Windows aceptó restaurar, pero la lectura no coincide con la matriz original."
                    .into(),
            );
        }
    }
    Ok(())
}
