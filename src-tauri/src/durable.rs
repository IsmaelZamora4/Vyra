use serde::Serialize;
use std::{fs, io::Write, path::Path};

pub(crate) fn json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    let mut file =
        fs::File::create(&temporary).map_err(|e| format!("Temporal no escribible: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("No se pudo sincronizar el diario: {e}"))?;
    drop(file);
    crate::replace_file(&temporary, path)
        .map_err(|e| format!("No se pudo reemplazar el diario: {e}"))
}
