use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tracing::info;

use crate::capture::captured_payload::CapturedPayload;
use crate::capture::combat_port_detector::CombatPortDetector;
use crate::capture::stream_assembler::StreamAssembler;
use crate::capture::stream_processor::StreamProcessor;
use crate::combat::data_storage::DataStorage;
use crate::combat::ping_tracker::PingTracker;
use crate::i18n::lookup::{NpcLookup, SkillLookup};
use crate::platform::window_detector;

/// Pre-lock combat signatures: a cheap gate deciding which packets are worth
/// running through the parser before a port is locked. The port only actually
/// locks when the parser extracts *real damage* (see `run`), so this gate just
/// limits parse attempts — it does not by itself decide the lock.
///
/// The game's per-record terminator `?? 00 36` had leading byte `0x06`
/// pre-2026-06; the June 2026 update changed it to `0x0E` (`06 00 36` ->
/// `0E 00 36`), which silently broke the old single-magic port detection while
/// leaving the parser itself unaffected. We accept both leading bytes so the
/// gate survives that transition.
const COMBAT_SIGNATURES: [&[u8]; 2] = [
    &[0x0E, 0x00, 0x36], // current (post June 2026) record terminator
    &[0x06, 0x00, 0x36], // legacy terminator (pre June 2026)
];

/// Les marques de la fiche de personnage : gardées avant verrouillage, mais
/// **sans jamais compter pour le verrouillage lui-même**.
///
/// La distinction est tout l'objet de cette constante. Le verrou doit rester
/// strict : c'est lui qui a évité de se verrouiller sur un service tiers du
/// poste (observé sur le port 16005 à froid), et il se mérite par une *cadence*
/// de signatures que seul le flux du jeu en monde produit. Décider de
/// **garder** un paquet est une autre question, et beaucoup moins risquée :
/// au pire on journalise un paquet de trop.
///
/// Mesuré le 06/10/2026, en capturant la même session avec un meter tiers qui
/// ne filtre pas :
///
/// ```text
/// opcode                 sur le câble   dans notre capture
/// 11 56  inventaire            2              absent
/// 56 36  Combat Power          6              absent
/// 49 36  PlayerStats           2              absent
/// ```
///
/// Ces paquets arrivent **seuls**, sans aucun enregistrement de combat, donc
/// sans signature — et le filtre d'avant-verrouillage les jetait tous. La fiche
/// s'en sortait par accident : elle voyage imbriquée dans de gros paquets qui,
/// eux, contiennent des enregistrements de combat. C'est pour cela que le nom
/// et le niveau remontaient au site, et jamais l'équipement.
const MARQUES_FICHE: [&[u8]; 5] = [
    &[0x33, 0x36], // PlayerInfo — la fiche
    &[0x11, 0x56], // Inventaire — l'équipement
    &[0x56, 0x36], // Combat Power
    &[0x00, 0x90], // Pets
    &[0x49, 0x36], // PlayerStats
];

/// Faut-il garder ce paquet alors que le port n'est pas encore verrouillé ?
///
/// Isolée parce que c'est elle qui décide si un paquet existe ou non pour le
/// reste du meter, et qu'elle ne devait pas rester invérifiable au fond d'une
/// boucle asynchrone. Garder n'est pas verrouiller : le verrou, lui, continue
/// de ne compter que les vraies signatures de combat.
fn garder_avant_verrou(data: &[u8]) -> bool {
    contains_any(data, &COMBAT_SIGNATURES) || contains_any(data, &MARQUES_FICHE)
}
/// How many signature-bearing server->client packets a single flow must produce
/// *within SIGNATURE_WINDOW_MS* before it may lock the port. The live game stream
/// emits the record terminator ~19x/sec even while idle (movement/heartbeat), so it
/// clears this in well under a second. A coincidental loopback service (e.g. a local
/// helper on port 16005 during a game cold-start) produces the 3-byte pattern only a
/// handful of times over minutes and never reaches the threshold inside the window —
/// so it can no longer hijack the lock. The window (vs. a plain running total) means
/// only a *high-rate* flow qualifies, not one that slowly drips coincidental matches.
const SIGNATURE_LOCK_THRESHOLD: u32 = 12;
/// Sliding window for the signature-rate lock; the THRESHOLD packets must land within
/// this span. Reset the per-flow count whenever the gap since the last hit exceeds it.
const SIGNATURE_WINDOW_MS: i64 = 3_000;
const TLS_CONTENT_TYPES: [u8; 4] = [0x14, 0x15, 0x16, 0x17];
const TLS_VERSIONS: [u8; 5] = [0x00, 0x01, 0x02, 0x03, 0x04];
const WINDOW_CHECK_STOPPED_MS: i64 = 10_000;
const WINDOW_CHECK_RUNNING_MS: i64 = 60_000;
const STALE_CONNECTION_MS: i64 = 120_000;
/// While no port is locked, how often to log what the capture is seeing. Before
/// the lock every gate is silent, so without this a meter that never locks
/// leaves a log that cannot say why.
const UNLOCKED_REPORT_MS: i64 = 30_000;

/// Per-device packet counts while unlocked, for the periodic report. It is
/// written when a packet arrives, so a capture that sees nothing at all stays
/// silent; the device list at startup covers that case.
#[derive(Default)]
struct UnlockedStats {
    /// device -> (packets, packets carrying a combat signature)
    by_device: HashMap<String, (u64, u64)>,
    /// Packets dropped because no AION2 window was found.
    no_window: u64,
}

impl UnlockedStats {
    fn note(&mut self, cap: &CapturedPayload) {
        let device = cap.device_name.clone().unwrap_or_else(|| "?".into());
        let entry = self.by_device.entry(device).or_default();
        entry.0 += 1;
        if contains_any(&cap.data, &COMBAT_SIGNATURES) {
            entry.1 += 1;
        }
    }

    fn report(&self, window_found: bool) -> String {
        let mut devices: Vec<_> = self.by_device.iter().collect();
        devices.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        let mut out = format!(
            "Not locked yet (last {} s): AION2 window {}",
            UNLOCKED_REPORT_MS / 1000,
            if window_found { "found" } else { "NOT found" }
        );
        if self.no_window > 0 {
            out += &format!(", {} packets ignored for that", self.no_window);
        }
        if !window_found {
            // What might have been the game, so the next log says why it was
            // not recognised (a localised title, a launcher, ...).
            let candidates = window_detector::describe_candidates();
            if candidates.is_empty() {
                out += " (no window or program mentions \"aion\")";
            } else {
                out += &format!(" (look-alikes: {})", candidates.join(" | "));
            }
        }
        for (device, (packets, marked)) in devices.iter().take(8) {
            out += &format!("; {}: {} packets, {} with game markers", device, packets, marked);
        }
        out
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Routes captured packets through port detection, filtering, and the parsing pipeline.
pub struct CaptureDispatcher {
    data_storage: Arc<DataStorage>,
    skill_lookup: Arc<SkillLookup>,
    npc_lookup: Arc<NpcLookup>,
    port_detector: Arc<CombatPortDetector>,
    ping_tracker: Arc<PingTracker>,
    dot_skill_ids: std::collections::HashSet<i32>,
    suspended: Arc<AtomicBool>,
}

impl CaptureDispatcher {
    pub fn new(
        data_storage: Arc<DataStorage>,
        skill_lookup: Arc<SkillLookup>,
        npc_lookup: Arc<NpcLookup>,
        port_detector: Arc<CombatPortDetector>,
        ping_tracker: Arc<PingTracker>,
    ) -> Self {
        Self {
            data_storage,
            skill_lookup,
            npc_lookup,
            port_detector,
            ping_tracker,
            dot_skill_ids: std::collections::HashSet::new(),
            suspended: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_dot_skill_ids(&mut self, ids: std::collections::HashSet<i32>) {
        self.dot_skill_ids = ids;
    }

    /// Share the "suspended" switch with whoever flips it (the header's
    /// suspend button, through `suspend_capture`). While it is on, captured
    /// packets are dropped, so nothing is counted and the fight timer stops.
    pub fn use_suspend_flag(&mut self, flag: Arc<AtomicBool>) {
        self.suspended = flag;
    }

    /// Run the dispatch loop, consuming packets from the channel.
    pub async fn run(&self, mut receiver: mpsc::Receiver<CapturedPayload>) {
        let mut assemblers: HashMap<(u16, u16), (StreamAssembler, StreamProcessor)> = HashMap::new();
        // Per-flow count of signature-bearing packets seen while still unlocked, used
        // for the no-combat-needed signature lock (see SIGNATURE_LOCK_THRESHOLD).
        // Per-flow signature-rate tracker: (count_in_window, last_hit_ms).
        let mut sig_hits: HashMap<(u16, u16), (u32, i64)> = HashMap::new();
        let mut last_window_check_ms: i64 = 0;
        let mut is_aion_running = false;
        // Le **titre** déjà journalisé, pas seulement « trouvée ou non ».
        //
        // N'en garder que le booléen laissait un angle mort : la fenêtre restant
        // trouvée en continu, rien n'était écrit, et le journal ne distinguait pas
        // « le titre ne s'est jamais vidé » de « il s'est vidé et la
        // reconnaissance par programme l'a rattrapé ». Constaté le 06/10/2026 en
        // vérifiant deux téléports de barbaxou : aucun verrou perdu, mais aucun
        // moyen de dire pourquoi.
        let mut window_logged: Option<Option<String>> = None;
        let mut unlocked_stats = UnlockedStats::default();
        let mut last_unlocked_report_ms = now_ms();
        // Ajout XIII NRV : voir plus bas. Reçues = ce qui franchit la fenêtre du
        // jeu ; traitées = ce qui atteint vraiment le lecteur.
        let mut compte_recues: u64 = 0;
        let mut compte_traitees: u64 = 0;
        let mut compte_octets: u64 = 0;
        let mut dernier_bilan_ms = now_ms();

        while let Some(cap) = receiver.recv().await {
            if self.suspended.load(Ordering::SeqCst) {
                continue;
            }

            // Check AION window
            let now = now_ms();
            let interval = if is_aion_running { WINDOW_CHECK_RUNNING_MS } else { WINDOW_CHECK_STOPPED_MS };
            if now - last_window_check_ms >= interval {
                last_window_check_ms = now;
                let title = window_detector::find_aion2_window_title();
                let running = title.is_some();
                if window_logged.as_ref() != Some(&title) {
                    match &title {
                        Some(t) => info!("AION2 window found: {:?}", t),
                        None => info!(
                            "No AION2 window found (looking for a title starting with \"AION2\", or a window owned by AION2.exe); packets are ignored until there is one"
                        ),
                    }
                    window_logged = Some(title.clone());
                }
                if !running && is_aion_running {
                    self.port_detector.reset();
                    self.ping_tracker.reset();
                    assemblers.clear();
                    sig_hits.clear();
                }
                is_aion_running = running;
            }

            // While unlocked, count what arrives on each device and report it
            // now and then, so a log from a meter that never locks says why.
            if self.port_detector.current_port().is_none() {
                unlocked_stats.note(&cap);
                if !is_aion_running {
                    unlocked_stats.no_window += 1;
                }
                if now - last_unlocked_report_ms >= UNLOCKED_REPORT_MS {
                    info!("{}", unlocked_stats.report(is_aion_running));
                    unlocked_stats = UnlockedStats::default();
                    last_unlocked_report_ms = now;
                }
            } else {
                last_unlocked_report_ms = now;
            }

            // Bilan toutes les trente secondes, une fois le flux verrouillé :
            // combien de morceaux sont arrivés, combien ont été lus, et à quel
            // débit. Un écart ici désigne nos filtres ; un journal muet côté
            // `pcap_stats` avec un écart ici désigne notre traitement.
            if self.port_detector.current_port().is_some() && now - dernier_bilan_ms >= 30_000 {
                let secondes = (now - dernier_bilan_ms) as f64 / 1000.0;
                info!(
                    "XIII NRV : {} morceaux reçus, {} lus ({} écartés par les filtres),                      {:.1} Mo en {:.0} s, soit {:.1} Mo/s",
                    compte_recues,
                    compte_traitees,
                    compte_recues.saturating_sub(compte_traitees),
                    compte_octets as f64 / 1e6,
                    secondes,
                    compte_octets as f64 / 1e6 / secondes.max(1.0)
                );
                // Et où la lecture d'un paquet de dégâts s'arrête. Un arrêt en
                // pleine chaîne emporte les coups qui suivaient dans le même
                // paquet : c'est l'hypothèse pour les coups manquants en groupe.
                info!(
                    "XIII NRV : lecture des dégâts — {}",
                    crate::capture::stream_processor::diag_arrets::bilan()
                );
                info!(
                    "XIII NRV : {}",
                    crate::capture::stream_processor::diag_arrets::bilan_cibles()
                );
                compte_recues = 0;
                compte_traitees = 0;
                compte_octets = 0;
                dernier_bilan_ms = now;
            }

            if !is_aion_running {
                continue;
            }

            // Stale connection check
            if is_aion_running && self.port_detector.current_port().is_some() {
                let last_parsed = self.port_detector.last_parsed_at_ms();
                if last_parsed > 0 && now - last_parsed > STALE_CONNECTION_MS {
                    info!("No packets parsed for {}ms, resetting lock", now - last_parsed);
                    self.port_detector.reset();
                    self.ping_tracker.reset();
                    assemblers.clear();
                    sig_hits.clear();
                }
            }

            let current_port = self.port_detector.current_port();
            let locked_device = self.port_detector.current_device();

            // Ajout XIII NRV : situer la perte. En solo nos totaux tombent au
            // chiffre près sur l'analyseur du jeu ; à cinq en donjon il manque
            // 18 % des dégâts et 13 % des coups (mesuré le 05/10/2026 sur deux
            // boss). Reste à savoir où ils disparaissent — avant nous, dans le
            // pilote de capture, ou chez nous, dans ces filtres.
            //
            // `pcap_stats` répond pour le pilote. Ces compteurs répondent pour
            // nous : ce qui entre dans le répartiteur et ce qui en ressort.
            compte_recues += 1;
            compte_octets += cap.data.len() as u64;

            // Device filter
            if let Some(ref dev) = locked_device {
                if !device_matches(dev, cap.device_name.as_deref()) {
                    continue;
                }
            }

            // Preferred device filter
            if current_port.is_none() {
                if let Some(ref pref) = self.port_detector.preferred_device() {
                    if !device_matches(pref, cap.device_name.as_deref()) {
                        continue;
                    }
                }
            }

            // Port filter
            if let Some(port) = current_port {
                if cap.src_port != port && cap.dst_port != port {
                    continue;
                }
            }

            // Feed to ping tracker — also marks connection alive to prevent stale reset
            if let Some(port) = current_port {
                let had_ping_before = self.ping_tracker.current_ping_ms();
                self.ping_tracker.on_packet(&cap, port);
                let has_ping_now = self.ping_tracker.current_ping_ms();
                // If a new ping was received, mark the connection as active
                if has_ping_now != had_ping_before {
                    self.port_detector.mark_packet_parsed();
                }
            }

            // Only parse server->client (src == locked port)
            if let Some(port) = current_port {
                if cap.src_port != port {
                    continue;
                }
            }

            // Pre-lock filters. Once a port is locked these checks are skipped
            // entirely (the port/direction filters above already gate traffic),
            // keeping the hot path cheap during heavy combat.
            let unlocked = current_port.is_none();
            if unlocked && looks_like_tls(&cap.data) {
                continue;
            }

            // Garder, ce n'est pas verrouiller. Un paquet qui porte une marque
            // de fiche est conservé même sans signature de combat ; il ne
            // compte pas pour autant dans la cadence qui décide du verrou.
            let porte_signature = contains_any(&cap.data, &COMBAT_SIGNATURES);
            if unlocked && !garder_avant_verrou(&cap.data) {
                continue;
            }

            // Log raw packet if packet logging is enabled
            crate::logging::logger::log_packet(&cap);
            // And keep it in memory for a while, so a boss fight can be shared
            // without packet logging having been on. See `share::ring`.
            crate::share::ring::record(cap.src_port, &cap.data);

            // Get or create assembler
            let a = cap.src_port.min(cap.dst_port);
            let b = cap.src_port.max(cap.dst_port);
            let key = (a, b);

            let (assembler, processor) = assemblers.entry(key).or_insert_with(|| {
                let mut proc = StreamProcessor::new(self.data_storage.clone(), self.skill_lookup.clone(), self.npc_lookup.clone());
                proc.set_dot_skill_ids(self.dot_skill_ids.clone());
                (StreamAssembler::new(), proc)
            });

            // **Seules les vraies signatures comptent pour le verrou.** Sans ce
            // `porte_signature`, les paquets de fiche qu'on vient de laisser
            // passer gonfleraient la cadence et pourraient verrouiller le port
            // sur un flux qui n'est pas le jeu — ce que le seuil existe
            // précisément pour empêcher.
            if unlocked && porte_signature {
                self.port_detector.register_candidate(cap.src_port, key, cap.device_name.as_deref());
                // Count this signature-bearing packet against its source port (the
                // signature only ever travels server->client). The lock decision is
                // made below, after process_chunk, so we don't touch `assemblers`
                // while the assembler for this flow is still borrowed.
                // Windowed signature rate: reset the count if too long since the last
                // hit, so only a sustained high-rate flow (the live game) accumulates.
                let now = now_ms();
                let slot = sig_hits.entry(key).or_insert((0, now));
                if now - slot.1 > SIGNATURE_WINDOW_MS {
                    slot.0 = 0;
                }
                slot.0 += 1;
                slot.1 = now;
            }

            // A flow locks the port only by sustaining the game's signature RATE
            // (SIGNATURE_LOCK_THRESHOLD hits within SIGNATURE_WINDOW_MS). We deliberately
            // do NOT lock on a single parsed-damage event any more: a coincidental
            // loopback service can momentarily misparse as "damage" and steal the lock
            // during a cold start (observed locking onto port 16005 instead of the game).
            // Real combat produces a flood of signatures too, so the rate gate covers
            // both idle and combat while staying robust. Spawns/names are still parsed
            // into the store pre-lock, so mobs seen before the first fight stay identified.
            // Ajout XIII NRV : horodater les coups à l'heure de **capture** du
            // morceau, et non à celle de son traitement. Un paquet retardé —
            // réseau chargé, réassemblage en attente — décalait sinon tous ses
            // coups : mesuré le 02/10/2026, huit coups horodatés onze secondes
            // après le dernier échange réel, étirant le combat de 151 à 162
            // secondes et abaissant le DPS de chacun de 9 %.
            compte_traitees += 1;
            processor.set_capture_time(cap.captured_at_ms);
            let parsed = assembler.process_chunk(&cap.data, processor);

            let signature_locked =
                unlocked && sig_hits.get(&key).map(|(c, _)| *c).unwrap_or(0) >= SIGNATURE_LOCK_THRESHOLD;
            if signature_locked && self.port_detector.current_port().is_none() {
                self.port_detector.confirm_candidate(cap.src_port, cap.dst_port, cap.device_name.as_deref());
                // On lock, GC the orphaned candidate assemblers (the relay's
                // duplicate external flows) so only the locked flow is processed.
                if self.port_detector.current_port().is_some() {
                    assemblers.retain(|k, _| *k == key);
                    sig_hits.clear();
                }
            }

            if parsed {
                self.port_detector.mark_packet_parsed();
            }
        }
    }
}

fn looks_like_tls(data: &[u8]) -> bool {
    if data.len() < 3 {
        return false;
    }
    let content_type = data[0];
    let major = data[1];
    let minor = data[2];
    TLS_CONTENT_TYPES.contains(&content_type) && major == 0x03 && TLS_VERSIONS.contains(&minor)
}

fn contains_bytes(data: &[u8], needle: &[u8]) -> bool {
    needle.len() <= data.len() && data.windows(needle.len()).any(|w| w == needle)
}

fn contains_any(data: &[u8], needles: &[&[u8]]) -> bool {
    needles.iter().any(|n| contains_bytes(data, n))
}

fn device_matches(locked: &str, packet_device: Option<&str>) -> bool {
    match packet_device {
        Some(d) if !d.trim().is_empty() => d.trim().eq_ignore_ascii_case(locked),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(device: &str, data: &[u8]) -> CapturedPayload {
        CapturedPayload {
            src_port: 1,
            dst_port: 2,
            data: data.to_vec(),
            device_name: Some(device.into()),
            captured_at_ms: 0,
            src_ip: None,
            dst_ip: None,
            tcp_seq: 0,
            tcp_ack: 0,
        }
    }

    /// Un paquet de fiche doit survivre au filtre d'avant-verrouillage.
    ///
    /// Mesuré le 06/10/2026 en capturant la même session avec un meter tiers qui
    /// ne filtre pas, sur la même machine et au même instant :
    ///
    ///     opcode                 sur le câble   dans notre capture
    ///     11 56  inventaire            2              absent
    ///     56 36  Combat Power          6              absent
    ///     49 36  PlayerStats           2              absent
    ///
    /// Ces paquets arrivent seuls, sans aucun enregistrement de combat, donc
    /// sans signature — et le filtre les jetait tous. L'équipement de barbaxou
    /// n'est jamais remonté au site de toute la journée pour cette seule raison,
    /// alors que notre décodeur le lit parfaitement : nourri de la capture non
    /// filtrée, il rend ses 22 pièces avec les bons enchantements.
    ///
    /// La fiche s'en sortait par accident : elle voyage imbriquée dans de gros
    /// paquets qui, eux, contiennent des enregistrements de combat.
    #[test]
    fn un_paquet_de_fiche_survit_au_filtre_davant_verrouillage() {
        // Un paquet d'inventaire tel qu'il arrive : la marque, pas de signature.
        let inventaire = [0x16u8, 0x11, 0x56, 0x01, 0x02, 0x03, 0x04];
        assert!(
            !contains_any(&inventaire, &COMBAT_SIGNATURES),
            "témoin : ce paquet ne porte bien aucune signature de combat"
        );
        assert!(
            garder_avant_verrou(&inventaire),
            "l'inventaire doit être gardé ; le jeter est ce qui a privé le site              de l'équipement toute la journée du 06/10/2026"
        );

        // Un paquet de combat ordinaire : gardé comme avant.
        let combat = [0x16u8, 0x04, 0x38, 0x0E, 0x00, 0x36, 0x01];
        assert!(
            garder_avant_verrou(&combat),
            "témoin : un paquet de combat doit continuer de passer"
        );

        // Et ce qui n'est ni l'un ni l'autre reste écarté : le filtre doit
        // toujours filtrer, sinon on journalise tout le trafic du poste.
        let quelconque = [0x17u8, 0x03, 0x01, 0x00, 0x42, 0x42, 0x42];
        assert!(
            !garder_avant_verrou(&quelconque),
            "un paquet sans rapport ne doit pas passer : le filtre protège aussi              la vie privée du membre, puisque ce qui passe est journalisé"
        );
    }

    #[test]
    fn unlocked_report_names_each_device_and_the_window() {
        let mut stats = UnlockedStats::default();
        stats.note(&cap("NordLynx Tunnel", &[0x0E, 0x00, 0x36, 0x01]));
        stats.note(&cap("NordLynx Tunnel", &[0x01, 0x02]));
        stats.note(&cap("Realtek", &[0x01]));
        stats.no_window = 3;
        let line = stats.report(false);
        assert!(line.contains("AION2 window NOT found, 3 packets ignored for that"), "{line}");
        assert!(line.contains("NordLynx Tunnel: 2 packets, 1 with game markers"), "{line}");
        assert!(line.contains("Realtek: 1 packets, 0 with game markers"), "{line}");
        assert!(line.find("NordLynx").unwrap() < line.find("Realtek").unwrap(), "busiest first");
    }
}
