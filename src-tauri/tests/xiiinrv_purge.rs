//! La purge des journaux de paquets efface des fichiers : on vérifie qu'elle
//! n'efface que ceux-là.
//!
//!     cargo test --test xiiinrv_purge -- --nocapture

use std::fs;
use std::time::{Duration, SystemTime};

use xiiinrv_meter_lib::logging::logger::purger_vieux_journaux;

/// Écrit un fichier et lui donne l'âge voulu.
fn fichier(dossier: &std::path::Path, nom: &str, jours: u64) {
    let chemin = dossier.join(nom);
    fs::write(&chemin, b"contenu").expect("ecriture");
    if jours > 0 {
        let quand = SystemTime::now() - Duration::from_secs(jours * 24 * 60 * 60);
        let f = fs::File::options().write(true).open(&chemin).expect("ouverture");
        f.set_times(fs::FileTimes::new().set_modified(quand)).expect("date");
    }
}

#[test]
fn nefface_que_les_vieux_journaux_de_paquets() {
    let dossier = std::env::temp_dir().join(format!(
        "xiiinrv_purge_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dossier).expect("dossier");

    // Des journaux de paquets, d'âges différents.
    fichier(&dossier, "packets_vieux.txt", 30);
    fichier(&dossier, "packets_limite.txt", 8);
    fichier(&dossier, "packets_recent.txt", 2);
    fichier(&dossier, "packets_dujour.txt", 0);
    // Et des fichiers qui ne sont pas des journaux de paquets : même très
    // anciens, ils ne doivent pas être touchés. Le dossier contient aussi les
    // réglages du membre et son jeton.
    fichier(&dossier, "settings.json", 90);
    fichier(&dossier, "debug.log", 90);
    fichier(&dossier, "packets_sans_extension", 90);

    purger_vieux_journaux(&dossier);

    let existe = |nom: &str| dossier.join(nom).exists();

    assert!(!existe("packets_vieux.txt"), "un journal de 30 jours doit partir");
    assert!(!existe("packets_limite.txt"), "un journal de 8 jours doit partir");
    assert!(existe("packets_recent.txt"), "un journal de 2 jours doit rester");
    assert!(existe("packets_dujour.txt"), "le journal du jour doit rester");

    assert!(existe("settings.json"), "les réglages ne sont pas un journal");
    assert!(existe("debug.log"), "le journal de débogage n'est pas un journal de paquets");
    assert!(
        existe("packets_sans_extension"),
        "sans l'extension .txt, ce n'est pas un journal de paquets"
    );

    let _ = fs::remove_dir_all(&dossier);
}

#[test]
fn un_dossier_absent_ne_fait_pas_tomber_le_meter() {
    // Appelée au démarrage : si le dossier n'existe pas encore, elle doit se
    // taire et non paniquer.
    purger_vieux_journaux(&std::env::temp_dir().join("xiiinrv_dossier_qui_nexiste_pas"));
}
