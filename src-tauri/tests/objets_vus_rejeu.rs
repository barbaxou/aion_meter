//! Rejoue la capture du 15/10/2026 (entrée en monde, récolte d'azpha, deux
//! monstres à âme, diamant) et vérifie les objets vus.
//!
//! Chemin donné par A2_REPLAY_CAPTURE ; sans lui, le test sort sans échouer.
//! L'état des objets est global : tout est dans un seul test, en séquence.

use std::collections::HashMap;
use std::sync::Arc;

use xiiinrv_meter_lib::capture::objets_vus::{instantane, Genre};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};

fn rejouer(chemin: &str, jusqu_a: &str) {
    let mut processor = StreamProcessor::new(
        Arc::new(DataStorage::new()),
        Arc::new(SkillLookup::new()),
        Arc::new(NpcLookup::new()),
    );
    let texte = std::fs::read_to_string(chemin).expect("capture lisible");
    let mut flux: HashMap<String, PacketAccumulator> = HashMap::new();
    for ligne in texte.lines() {
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let mut p = ligne.splitn(3, '|');
        let (Some(ts), Some(cle), Some(hex)) = (p.next(), p.next(), p.next()) else { continue };
        if !jusqu_a.is_empty() && ts >= jusqu_a {
            break;
        }
        if hex.len() % 2 != 0 {
            continue;
        }
        let Ok(octets) = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
        else {
            continue;
        };
        let acc = flux.entry(cle.to_string()).or_insert_with(PacketAccumulator::new);
        acc.append(&octets);
        let consommes = processor.consume_stream(acc.snapshot());
        if consommes > 0 {
            acc.discard_bytes(consommes);
        }
    }
}

fn ephemeres() -> usize {
    instantane().iter().filter(|o| o.genre == Genre::Ephemere).count()
}

#[test]
fn les_objets_de_la_capture_du_15_10_sont_vus() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };

    // 12:27:40 : un éphémère créé à 12:27:39 (id 0xa43f) n'est pas encore retiré,
    // et celui de 12:27:40,78 n'est pas encore là. Il y en a donc un seul.
    rejouer(&chemin, "2026-10-15T12:27:40");
    assert_eq!(ephemeres(), 1, "un seul objet éphémère à 12:27:40");

    // 12:27:48 : les six éphémères vus entre 12:27:39 et 12:27:45 ont tous leur
    // retrait (12:27:41 à 12:27:47). Rejouer depuis le début repasse par le
    // téléport de 12:27:13, qui vide l'état.
    rejouer(&chemin, "2026-10-15T12:27:48");
    assert_eq!(ephemeres(), 0, "tous les éphémères sont retirés à 12:27:48");

    // Le décor reste : les `34 36` de l'arrivée (12:27:13) et du diamant (12:30:03 à 06).
    rejouer(&chemin, "");
    let decor: Vec<_> = instantane().into_iter().filter(|o| o.genre == Genre::Decor).collect();
    assert!(decor.len() >= 4, "au moins 4 objets de décor, trouvés : {}", decor.len());
    assert!(decor.iter().all(|o| o.modele != 0 && o.x.abs() < 450_000.0 && o.y.abs() < 450_000.0));
    // Les monstres : la capture en contient 43 de catégorie 4, 5 ou 12, à 10 m près.
    let mobs: Vec<_> = instantane().into_iter().filter(|o| o.genre == Genre::Mob).collect();
    assert!(mobs.len() >= 25, "au moins 25 points de monstres, trouvés : {}", mobs.len());
    assert!(mobs.iter().all(|o| o.modele != 0 && o.x.abs() < 450_000.0 && o.y.abs() < 450_000.0));
    assert!(mobs.iter().any(|o| o.modele == 2_950_007), "le type 2950007 doit être vu");
    assert!(mobs.iter().any(|o| o.modele == 2_950_139), "le type 2950139 (catégorie 12) doit être vu");
    // Le modèle 1300908 de l'arrivée de 12:27:13.
    assert!(decor.iter().any(|o| o.modele == 1_300_908), "le modèle 1300908 doit être vu");
}
