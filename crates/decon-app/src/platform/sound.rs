//! Воспроизведение звуков оповещения (S2.5): зонды, сигналы сессии,
//! повторы-«будильники».
//!
//! Файлы — процедурный синтез (`scripts/gen_sounds.py`, CC0, без сторонних
//! сэмплов). Путь: assets рядом с бинарником (dev) / текущий каталог.
//!
//! Платформы: Linux — первый доступный из canberra-gtk-play / paplay /
//! aplay (вызов через std::process::Command, без аллокаций в hot path —
//! звук не hot path); Windows — winmm PlaySound (SND_ASYNC). Ошибки
//! проглатываются: звук — мягкая функция, headless/CI — тишина.

use std::path::PathBuf;

/// Имя файла звука по выбору пользователя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// Мягкий перезвон (`chime.wav`).
    Chime,
    /// Колокол (`bell.wav`).
    Bell,
}

impl Sound {
    /// Имя файла в assets.
    pub const fn file(self) -> &'static str {
        match self {
            Sound::Chime => "chime.wav",
            Sound::Bell => "bell.wav",
        }
    }
}

/// Найти WAV в assets: рядом с бинарником, затем assets/ от cwd.
pub fn sound_path(sound: Sound) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidates = [
        exe_dir.join("assets").join(sound.file()),
        exe_dir.join(sound.file()),
        PathBuf::from("assets").join(sound.file()),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Проиграть звук (асинхронно, неблокирующе: звук не держит UI-поток).
///
/// Тишина без ошибок: нет файла / нет плеера / headless — не повод
/// ломать поток зондов (уведомление ОС всё равно ушло).
pub fn play(sound: Sound) {
    let Some(path) = sound_path(sound) else {
        return;
    };
    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};
        let path_str = path.to_string_lossy().into_owned();
        // canberra — тема-осведомлённый плеер GNOME; paplay — PulseAudio;
        // aplay — ALSA-фолбэк. Первый найденный в PATH выигрывает.
        let players = [
            ("canberra-gtk-play", vec!["-f", &path_str]),
            ("paplay", vec![&path_str]),
            ("aplay", vec!["-q", &path_str]),
        ];
        for (bin, args) in players {
            let ok = Command::new(bin)
                .args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|mut child| {
                    // Не зомбируем процесс: дочерний умрёт сам, но reapнем
                    // в фоне через detach — простейший способ не блокировать
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    true
                })
                .unwrap_or(false);
            if ok {
                return;
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        // winmm PlaySound: SND_ASYNC (0x1) — вернуться сразу; SND_FILENAME
        // (0x20000) — первый аргумент это путь к файлу, а не системный
        // алиас или ресурс. Символы winmm.dll, новых зависимостей нет.
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn PlaySoundW(
                psz_sound: *const u16,
                hmod: *mut core::ffi::c_void,
                fdw_sound: u32,
            ) -> i32;
        }
        let wide: Vec<u16> = path
            .as_os_str()
            .to_str()
            .map(|s| s.encode_utf16().chain(std::iter::once(0)).collect())
            .unwrap_or_default();
        if !wide.is_empty() {
            // SND_ASYNC | SND_FILENAME = 0x1 | 0x20000
            unsafe { PlaySoundW(wide.as_ptr(), core::ptr::null_mut(), 0x20001) };
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = path; // macOS и прочие — тишина до появления таргета
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_files_exist_in_assets() {
        // Критерий 4c: WAV-файлы в репозитории, не пустые, валидный RIFF.
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for sound in [Sound::Chime, Sound::Bell] {
            let path = manifest.join("assets").join(sound.file());
            assert!(path.is_file(), "нет файла {}", path.display());
            let bytes = std::fs::read(&path).expect("читается");
            assert!(bytes.len() > 1000, "файл подозрительно мал");
            assert_eq!(&bytes[0..4], b"RIFF", "RIFF-заголовок");
            assert_eq!(&bytes[8..12], b"WAVE", "WAVE-подпись");
        }
    }

    #[test]
    fn play_never_panics_headless() {
        // CI/headless: плееров нет — тихий no-op, не паника.
        play(Sound::Chime);
        play(Sound::Bell);
    }
}
