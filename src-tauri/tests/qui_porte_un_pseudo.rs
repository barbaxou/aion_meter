//! Quelles entités portent un pseudo, et lesquelles restent des numéros ?
//!
//! Le 08/10/2026, l'overlay affichait `#9635` et `#532` au lieu de deux pseudos,
//! dans un groupe de quatre. Les deux étaient bien des joueurs : ils encaissaient
//! des coups, et l'un d'eux possédait une invocation (« Summon 17819 linked to
//! owner 9635 »).
//!
//! Au même moment, le journal ne comptait **aucun** effectif complet :
//! 1 611 paquets de composition de groupe, tous `complete=false`. Il faut
//! départager deux explications :
//!
//! 1. le pseudo de ces entités n'arrive jamais dans le flux ;
//! 2. il arrive et notre lecture le laisse passer.
//!
//! Ce rejeu lit une capture réelle dans le vrai chemin, puis dit qui a un
//! pseudo, qui n'en a pas, et ce que l'effectif du groupe apporte.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test qui_porte_un_pseudo -- --ignored --nocapture

use std::collections::HashMap;
use std::sync::Arc;

use xiiinrv_meter_lib::capture::stream_assembler::StreamAssembler;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};

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
fn qui_porte_un_pseudo_dans_cette_capture() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    let storage = Arc::new(DataStorage::new());
    let mut flux: HashMap<String, (StreamAssembler, StreamProcessor)> = HashMap::new();
    let mut lignes = 0usize;

    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
        if bouts.len() != 3 {
            continue;
        }
        let Some(donnees) = decode_hex(bouts[2]) else { continue };
        lignes += 1;
        let (assembleur, processeur) = flux.entry(bouts[1].to_string()).or_insert_with(|| {
            (
                StreamAssembler::new(),
                StreamProcessor::new(
                    storage.clone(),
                    Arc::new(SkillLookup::new()),
                    Arc::new(NpcLookup::new()),
                ),
            )
        });
        assembleur.process_chunk(&donnees, processeur);
    }

    let pseudos = storage.get_nicknames();
    let combat = storage.get_combat_snapshot_light();
    let effectif = storage.get_party_members();

    println!("\n{lignes} lignes rejouées depuis {chemin}");

    println!("\neffectif du groupe tel que nous le lisons ({}) :", effectif.len());
    for (nom, m) in &effectif {
        println!(
            "  {nom:<18} niveau {:>3}  score {:>5}  CP {:>8}  serveur {}",
            m.level, m.gear_score, m.combat_power, m.server_id
        );
    }

    // Tous ceux qui ont porté un coup : ce sont eux qui ont une ligne.
    let mut frappeurs: HashMap<i32, i64> = HashMap::new();
    for td in combat.values() {
        for (&acteur, ad) in &td.actors {
            *frappeurs.entry(acteur).or_default() += ad.total_damage;
        }
    }
    let mut classes: Vec<(i32, i64)> = frappeurs.into_iter().collect();
    classes.sort_by_key(|(_, d)| std::cmp::Reverse(*d));

    println!("\nceux qui ont porté des coups, et leur pseudo :");
    println!("  {:>9}  {:>12}  {:<20}  {}", "entité", "dégâts", "pseudo", "invocation de");
    let mut sans_pseudo = 0usize;
    for (id, degats) in classes.iter().take(25) {
        let nom = pseudos.get(id).cloned().unwrap_or_default();
        if nom.is_empty() {
            sans_pseudo += 1;
        }
        let proprietaire = storage
            .get_summon_data()
            .get(id)
            .map(|o| o.to_string())
            .unwrap_or_else(|| "—".to_string());
        println!(
            "  {id:>9}  {degats:>12}  {:<20}  {proprietaire}",
            if nom.is_empty() { "— (affiché #id)" } else { nom.as_str() }
        );
    }
    println!(
        "\n  {sans_pseudo} frappeurs sans pseudo sur les {} affichés",
        classes.len().min(25)
    );

    println!("\n{} pseudos connus au total :", pseudos.len());
    let mut tous: Vec<(&i32, &String)> = pseudos.iter().collect();
    tous.sort_by_key(|(id, _)| **id);
    for (id, nom) in tous.iter().take(30) {
        println!("  {id:>9}  {nom}");
    }

    println!(
        "\n  Si un frappeur sans pseudo porte un nom dans l'effectif du groupe,\n  \
         c'est que les deux sources existent et que rien ne les relie."
    );
}
