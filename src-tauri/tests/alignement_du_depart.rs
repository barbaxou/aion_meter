//! Le nombre de paquets dépend-il de l'octet où l'on commence à lire ?
//!
//! Le 07/10/2026, nos deux captures enregistraient le **même flux** — mêmes
//! morceaux TCP, même plafond à 1 428 octets, 301 936 octets chez un meter
//! tiers contre 304 811 chez nous sur la même fenêtre, et nos premières lignes
//! sont les siennes décalées de deux.
//!
//! Pourtant, en appliquant **notre** découpage aux deux, nous tirions 1 073
//! paquets des nôtres et 8 570 des siennes. Même code, même flux : il ne reste
//! que le point de départ.
//!
//! Ce diagnostic essaie chaque décalage de départ et compte ce qui en sort. Si
//! un décalage rend soudain des milliers de petits paquets, le flux est sain et
//! nous lisons simplement à côté — et il faudra savoir se recaler. Si tous les
//! décalages se valent, l'explication est ailleurs.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... A2_DE=22:02:00 A2_A=22:05:00 \
//!     cargo test --test alignement_du_depart -- --ignored --nocapture

use xiiinrv_meter_lib::capture::framing::{walk, FrameKind};

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[test]
#[ignore = "diagnostic"]
fn le_depart_change_t_il_le_decoupage() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let de = std::env::var("A2_DE").unwrap_or_else(|_| "00:00:00".to_string());
    let a = std::env::var("A2_A").unwrap_or_else(|_| "23:59:59".to_string());
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    // Le flux du plus gros émetteur, d'un seul tenant : on cherche l'effet du
    // point de départ, pas celui du réassemblage.
    let mut par_cle: std::collections::HashMap<String, Vec<u8>> = Default::default();
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
        if bouts.len() != 3 {
            continue;
        }
        let heure = &bouts[0][11..19.min(bouts[0].len())];
        if heure < de.as_str() || heure > a.as_str() {
            continue;
        }
        let Some(octets) = decode_hex(bouts[2]) else { continue };
        par_cle.entry(bouts[1].to_string()).or_default().extend(octets);
    }
    let Some((cle, flux)) = par_cle.into_iter().max_by_key(|(_, v)| v.len()) else {
        println!("aucune donnée dans la fenêtre");
        return;
    };
    println!("\n{chemin}\n  flux {cle}, {} octets, fenêtre {de} → {a}", flux.len());
    println!("\n  départ   paquets   dont <256   octets rendus   plus gros");

    for depart in 0..24usize {
        if depart >= flux.len() {
            break;
        }
        let d = walk(&flux[depart..]);
        let mut petits = 0usize;
        let mut octets = 0usize;
        let mut gros = 0usize;
        for f in &d.frames {
            if f.kind != FrameKind::Packet {
                continue;
            }
            let t = f.end - f.start;
            octets += t;
            gros = gros.max(t);
            if t < 256 {
                petits += 1;
            }
        }
        println!(
            "  {depart:>6}   {:>7}   {petits:>9}   {octets:>13}   {gros:>9}",
            d.frames.len()
        );
    }
    println!(
        "\n  Un décalage qui rend soudain des milliers de petits paquets signifie\n  \
         que le flux est sain et que nous lisons à côté."
    );
}
