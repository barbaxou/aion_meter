//! Finding the AION 2 window on Windows.
//!
//! By title first (`AION2…`), which is how the meter always did it. Failing
//! that, by the program that owns the window (`AION2.exe`): a player's log
//! showed ~600 game packets every 30 s ignored because his game window was not
//! titled `AION2…` (a localised or wrapped client), so the title alone is not
//! enough.

use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

fn is_game_title(title: &str) -> bool {
    title.starts_with("AION2")
}

fn is_game_exe(path: &str) -> bool {
    path.rsplit(['\\', '/']).next().is_some_and(|name| name.eq_ignore_ascii_case("AION2.exe"))
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..len])
}

/// The full path of the program that owns `hwnd`. Needs no elevation for the
/// player's own processes.
fn exe_path(hwnd: HWND) -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let windows = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        windows.push(hwnd);
        BOOL(1)
    }
    let mut windows: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut Vec<HWND> as isize));
    }
    windows
}

/// La fenêtre décrite est-elle celle du jeu, et sous quel libellé la nommer ?
///
/// Décision isolée dans une fonction pure : c'est elle qui décide si le meter
/// lit ou jette les paquets, et elle ne doit pas rester invérifiable derrière
/// Win32. Le libellé rendu ne sert qu'au journal — l'appelant ne regarde que
/// `is_some()`.
fn reconnait(titre: &str, exe: Option<&str>) -> Option<String> {
    match exe {
        // Le programme est lisible : c'est lui qui tranche, quel que soit le
        // titre. Un titre vide n'est **pas** un motif de rejet — le jeu vide le
        // sien pendant les chargements et les téléports, et le refuser là
        // coûtait au meter son verrou de port et son réassemblage, donc la
        // capture, pour une à deux minutes à chaque fois.
        Some(p) if is_game_exe(p) => Some(if titre.is_empty() {
            // Un libellé parlant pour le journal, puisqu'il n'y a pas de titre.
            format!("(sans titre) {}", p.rsplit(['\\', '/']).next().unwrap_or(p))
        } else {
            titre.to_string()
        }),

        // Un autre programme, **même si son titre commence par « AION2 »**.
        //
        // Le 06/10/2026, le meter a annoncé « AION2 window found: "AION2 DPS
        // Meter" » : la fenêtre de Kuroukihime, pas le jeu. La reconnaissance
        // se faisait d'abord par `title.starts_with("AION2")`, sur la première
        // fenêtre venue, et trois fenêtres du poste commençaient ainsi — celle
        // du jeu, celle de ce meter et sa vue web. Conséquence : `is_aion_running`
        // restait vrai alors que le jeu était fermé, donc la remise à zéro du
        // détecteur de port et des réassembleurs ne se déclenchait jamais, et
        // l'état périmé passait dans la connexion suivante.
        Some(_) => None,

        // Programme illisible — le titre est alors tout ce qu'on a. Cela
        // n'arrive pas pour les processus du joueur lui-même, mais mieux vaut
        // un repli que l'aveuglement.
        None => is_game_title(titre).then(|| titre.to_string()),
    }
}

/// Find the AION 2 game window and return its title, or None if not found.
pub fn find_aion2_window_title() -> Option<String> {
    // Un seul passage, et c'est `reconnait` qui décide à chaque fenêtre.
    //
    // Il y avait auparavant un premier balayage par titre seul, qui retenait la
    // première fenêtre dont le titre commençait par « AION2 ». Il désignait la
    // fenêtre d'un autre meter aussi volontiers que celle du jeu — constaté le
    // 06/10/2026 — et il court-circuitait le contrôle du programme.
    top_level_windows().into_iter().find_map(|h| {
        if !unsafe { IsWindowVisible(h) }.as_bool() {
            return None;
        }
        reconnait(&window_title(h), exe_path(h).as_deref())
    })
}

/// Check if the AION 2 game window is currently running.
pub fn find_aion2_window() -> bool {
    find_aion2_window_title().is_some()
}

/// Check if the foreground window belongs to AION 2, by title or by program.
pub fn is_aion2_foreground() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return false;
    }
    is_game_title(&window_title(hwnd)) || exe_path(hwnd).is_some_and(|p| is_game_exe(&p))
}

/// Windows that look like they might be the game (title or program mentions
/// "aion"), for the log when none is recognised. Deliberately not every window:
/// other titles (browser tabs above all) are none of the meter's business.
pub fn describe_candidates() -> Vec<String> {
    let mut out = Vec::new();
    for h in top_level_windows() {
        let title = window_title(h);
        let exe = exe_path(h).unwrap_or_default();
        let exe_name = exe.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        if title.to_lowercase().contains("aion") || exe_name.to_lowercase().contains("aion") {
            let visible = unsafe { IsWindowVisible(h) }.as_bool();
            out.push(format!("title={title:?} exe={exe_name:?} visible={visible}"));
            if out.len() >= 8 {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_game_by_title_or_program() {
        assert!(is_game_title("AION2"));
        assert!(is_game_title("AION2 | Amber1"));
        assert!(!is_game_title("아이온2"));
        assert!(is_game_exe(r"D:\SteamLibrary\steamapps\common\AION2\Aion2\Binaries\Win64\AION2.exe"));
        assert!(is_game_exe(r"C:\Games\aion2.EXE"));
        assert!(!is_game_exe(r"C:\Games\AION2Launcher.exe"));
        assert!(!is_game_exe(""));
    }

    /// Une fenêtre du jeu au titre vide doit être reconnue à son programme.
    ///
    /// Le 06/10/2026, le journal de barbaxou montrait le meter aveugle 2 min 22 s
    /// — « No AION2 window found », verrou de port perdu, réassembleurs vidés —
    /// avec, en face, la fenêtre rejetée nommée dans son propre relevé :
    /// `title="" exe="AION2.exe"`. C'était bien le jeu. La branche de secours,
    /// censée reconnaître « une fenêtre appartenant à AION2.exe », ressortait
    /// avant d'avoir regardé le programme, au seul motif que le titre était
    /// vide — ce que le jeu fait pendant les chargements et les téléports.
    ///
    /// Ce `return None` n'existait que pour avoir une chaîne non vide à
    /// journaliser. Une commodité de type de retour, payée par la perte de la
    /// capture.
    #[test]
    fn une_fenetre_du_jeu_sans_titre_est_reconnue() {
        let jeu = Some(r"D:\1-Jeux\AION2_TW\Aion2\Binaries\Win64\AION2.exe");

        // Par le titre : le cas ordinaire, qui doit continuer de marcher.
        assert!(reconnait("AION2  ", None).is_some(), "témoin : reconnaissance par le titre");

        // Par le programme, titre vide : le cas du 06/10/2026.
        assert!(
            reconnait("", jeu).is_some(),
            "une fenêtre d'AION2.exe au titre vide est le jeu pendant un chargement ;              la rejeter fait perdre le verrou de port et le réassemblage"
        );

        // Et ce qui ne doit pas être pris pour le jeu.
        assert!(reconnait("", Some(r"C:\Windows\explorer.exe")).is_none(), "autre programme, titre vide");
        assert!(reconnait("Discord", Some(r"C:\Discord\Discord.exe")).is_none(), "autre programme");
        assert!(reconnait("", None).is_none(), "rien du tout");
    }

    /// La fenêtre d'un autre meter ne doit pas être prise pour le jeu.
    ///
    /// Le 06/10/2026, le journal annonçait `AION2 window found: "AION2 DPS
    /// Meter"` — la fenêtre de Kuroukihime. Trois fenêtres visibles du poste
    /// commençaient par « AION2 » :
    ///
    /// ```text
    /// AION2 DPS Meter   AionDpsMeter.UI    (un autre meter)
    /// AION2             AION2              (le jeu)
    /// AION2 DPS Meter   msedgewebview2     (sa vue web)
    /// ```
    ///
    /// La reconnaissance se faisait par `title.starts_with("AION2")` sur la
    /// première venue. Elle désignait donc un autre meter, et `is_aion_running`
    /// restait vrai alors que le jeu était fermé : la remise à zéro du détecteur
    /// de port et des réassembleurs ne se déclenchait plus, et l'état périmé
    /// passait dans la connexion suivante.
    #[test]
    fn un_autre_meter_nest_pas_le_jeu() {
        let kurou = Some(r"D:\9 - meters aion\kuroukihime\AionDpsMeter.UI.exe");
        let webview = Some(r"C:\Program Files\Microsoft\EdgeWebView\msedgewebview2.exe");
        let jeu = Some(r"D:\1-Jeux\AION2_TW\Aion2\Binaries\Win64\AION2.exe");

        assert!(
            reconnait("AION2 DPS Meter", kurou).is_none(),
            "la fenêtre d'un autre meter commence par « AION2 » sans être le jeu"
        );
        assert!(
            reconnait("AION2 DPS Meter", webview).is_none(),
            "sa vue web non plus"
        );

        // Témoins : le jeu doit continuer d'être reconnu, par le programme.
        assert_eq!(
            reconnait("AION2", jeu).as_deref(),
            Some("AION2"),
            "témoin : le vrai jeu, titré"
        );
        assert!(
            reconnait("", jeu).is_some(),
            "témoin : le vrai jeu, titre vidé pendant un chargement"
        );

        // Et le repli quand le programme est illisible : le titre décide seul.
        assert_eq!(
            reconnait("AION2", None).as_deref(),
            Some("AION2"),
            "programme illisible : mieux vaut le titre que l'aveuglement"
        );
        assert!(
            reconnait("Discord", None).is_none(),
            "programme illisible et titre quelconque"
        );
    }
}
