//! Notre propre réassemblage du flux du jeu.
//!
//! POURQUOI ON NE SE SERT PAS DE CELUI D'A2TOOLS
//!   On a d'abord lu les paquets là où A2Tools les avait déjà découpés, en
//!   ajoutant une ligne dans son `parse_perfect_packet`. Ça marchait la plupart
//!   du temps, mais pas toujours, et le 29/09/2026 le journal des paquets a
//!   montré pourquoi. Sur une entrée en jeu, le jeu avait envoyé les trois
//!   paquets du défilement du Combat Power (77 148 → 111 394 → 132 462) et
//!   l'inventaire complet (13 732 octets). Le meter n'a vu que le premier
//!   Combat Power. Les mêmes octets, rejoués dans notre propre découpeur,
//!   rendaient les quatre paquets.
//!
//!   Le décodage n'a donc jamais été en cause : c'était le chemin par lequel les
//!   paquets nous arrivaient. On se branche maintenant sur les morceaux bruts,
//!   avant tout découpage, et on réassemble nous-mêmes. Notre lecture ne dépend
//!   plus du réassembleur d'A2Tools, et son code ne porte plus une seule ligne de
//!   nous.
//!
//! LE PROBLÈME DU DÉBUT
//!   A2Tools ne connaît pas d'avance le port du jeu : il l'apprend en voyant
//!   passer assez de paquets de combat. L'inventaire et le Combat Power arrivent
//!   autour de ce moment-là. On garde donc de côté ce qui défile avant le
//!   verrouillage, et au verrouillage on reprend le flux retenu par A2Tools —
//!   celui du jeu, sans ambiguïté — recalé sur un vrai début de paquet.
//!
//!   Deux essais avant celui-ci, tous deux dans le filtre d'A2Tools, tous deux
//!   faux : ne laisser passer que les morceaux portant un de nos opcodes (un gros
//!   paquet est découpé par le réseau, seul le premier morceau le porte), puis
//!   retenir tout un flux dès qu'un opcode y apparaît (ces octets se retrouvent
//!   par hasard ailleurs — on avait retenu les flux 55987 et 64270 alors que le
//!   jeu parlait sur 61944).

use parking_lot::Mutex;
use std::sync::OnceLock;
use tracing::info;

use super::collecte::{lecture_ouverte, observer};

// Plafonds du tampon d'avant le verrouillage : si le port ne se verrouille
// jamais, il ne doit pas grossir indéfiniment. Ils sont posés **par sens de
// connexion** et non seulement au total : sur une machine qui passe par un
// relais, plusieurs connexions locales défilent avant le verrouillage, et un
// unique plafond global pourrait être rempli par ce bruit avant même que le flux
// du jeu n'apparaisse. L'entrée en jeu tient très largement dans un sens.
const PLAFOND_PAR_SENS: usize = 3 * 1024 * 1024;
const PLAFOND_TOTAL: usize = 12 * 1024 * 1024;
const SENS_SUIVIS_AU_PLUS: usize = 24;

/// Quand le flux du jeu est suivi, un paquet incomplet fait attendre la suite.
/// Si l'attente dépasse cette taille sans que rien ne soit consommé, c'est que le
/// découpage s'est perdu : on se recale plutôt que d'attendre pour rien. Sans ce
/// garde-fou, un blocage a fait perdre la fiche et les pets d'une entrée en jeu
/// entière alors qu'ils étaient bien arrivés.
const ATTENTE_MAXIMALE: usize = 256 * 1024;

/// Port d'origine et port de destination, **dans cet ordre**. Avant le
/// verrouillage, les deux sens de la connexion défilent ici : les mêler dans un
/// seul tampon rendrait le découpage illisible. Chaque sens a donc le sien, et
/// on ne suit que celui qui va du serveur vers le client.
type Flux = (u16, u16);

#[derive(Default)]
struct Suivi {
    /// Avant le verrouillage : on garde sans rien lire, faute de savoir quel
    /// flux est celui du jeu.
    avant: Vec<(Flux, Vec<u8>)>,
    /// Après le verrouillage : le flux du jeu, lu au fil de l'eau. Ce qui reste
    /// est le début d'un paquet dont la suite n'est pas encore arrivée.
    apres: Option<(Flux, Vec<u8>)>,
}

static SUIVI: OnceLock<Mutex<Suivi>> = OnceLock::new();

fn suivi() -> &'static Mutex<Suivi> {
    SUIVI.get_or_init(|| Mutex::new(Suivi::default()))
}

/// Un morceau de flux, tel que la carte réseau l'a vu — avant tout découpage.
///
/// Tant que le port n'est pas verrouillé on ne fait que garder : on ne sait pas
/// encore quel flux est celui du jeu, et lire au hasard remonterait la fiche d'on
/// ne sait quoi. Une fois le flux connu, chaque morceau est réassemblé et lu
/// immédiatement.
pub fn recevoir(origine: u16, destination: u16, donnees: &[u8]) {
    if !lecture_ouverte() || donnees.is_empty() {
        return;
    }
    let cle = (origine, destination);
    let mut suivi = suivi().lock();

    if let Some((suivie, tampon)) = suivi.apres.as_mut() {
        if *suivie != cle {
            return;
        }
        tampon.extend_from_slice(donnees);
        // On lit en tenant le verrou : `observer` ne rappelle jamais ce module,
        // donc il n'y a pas de tour de verrou possible, et le flux reste dans
        // l'ordre où il est arrivé.
        avancer(tampon);
        return;
    }

    let total: usize = suivi.avant.iter().map(|(_, o)| o.len()).sum();
    if total + donnees.len() > PLAFOND_TOTAL {
        return;
    }
    match suivi.avant.iter_mut().find(|(k, _)| *k == cle) {
        Some((_, octets)) => {
            if octets.len() + donnees.len() <= PLAFOND_PAR_SENS {
                octets.extend_from_slice(donnees);
            }
        }
        None => {
            if suivi.avant.len() < SENS_SUIVIS_AU_PLUS {
                suivi.avant.push((cle, donnees.to_vec()));
            }
        }
    }
}

/// Le port vient d'être verrouillé : `origine` est le port du jeu. On reprend ce
/// qu'on avait gardé pour ce sens, on le recale sur un vrai début de paquet, on
/// le lit, et on garde la suite du flux à l'œil.
pub fn verrouille(origine: u16, destination: u16) {
    if !lecture_ouverte() {
        return;
    }
    let cle = (origine, destination);
    let garde = {
        let mut suivi = suivi().lock();
        let garde = suivi
            .avant
            .iter()
            .position(|(k, _)| *k == cle)
            .map(|i| suivi.avant.swap_remove(i).1)
            .unwrap_or_default();
        suivi.avant.clear();
        garde
    };

    let mut tampon = match debut_aligne(&garde) {
        Some(debut) => {
            info!(
                "XIII NRV : flux {:?} verrouillé, {} octets gardés, début aligné à {}",
                cle,
                garde.len(),
                debut
            );
            garde[debut..].to_vec()
        }
        None => {
            info!(
                "XIII NRV : flux {:?} verrouillé, {} octets gardés mais aucun début de paquet reconnaissable : on repart de la suite",
                cle,
                garde.len()
            );
            Vec::new()
        }
    };
    avancer(&mut tampon);
    suivi().lock().apres = Some((cle, tampon));
}

/// La connexion au jeu est tombée : on oublie le flux suivi.
pub fn deverrouille() {
    let mut suivi = suivi().lock();
    suivi.apres = None;
    suivi.avant.clear();
}

/// Le partage vient d'être coupé : on ne garde rien.
pub fn oublier() {
    *suivi().lock() = Suivi::default();
}

/// Sort du tampon tous les paquets complets et les donne à lire, puis ne garde
/// que le début du paquet suivant.
fn avancer(tampon: &mut Vec<u8>) {
    let consommes = extraire(tampon);
    if consommes > 0 {
        tampon.drain(..consommes);
        return;
    }
    // Rien n'a été consommé. Tant que le tampon reste petit c'est normal : on
    // attend la suite d'un paquet. Passé la limite, c'est que le découpage s'est
    // perdu, et attendre davantage ne ferait qu'empirer les choses.
    if tampon.len() < ATTENTE_MAXIMALE {
        return;
    }
    match debut_aligne(tampon) {
        Some(debut) if debut > 0 => {
            info!("XIII NRV : découpage bloqué, recalé de {} octets", debut);
            tampon.drain(..debut);
            let consommes = extraire(tampon);
            tampon.drain(..consommes);
        }
        _ => {
            info!(
                "XIII NRV : découpage bloqué sur {} octets illisibles, on repart de zéro",
                tampon.len()
            );
            tampon.clear();
        }
    }
}

/// Parcourt le tampon et donne à lire chaque paquet complet. Renvoie le nombre
/// d'octets consommés : le reste est le début d'un paquet à compléter.
///
/// Différence avec `parcourir` : ici on s'arrête net dès qu'un paquet est
/// incomplet, au lieu de chercher à se rattraper. La suite arrivera au prochain
/// morceau — c'est tout l'intérêt d'accumuler.
fn extraire(tampon: &[u8]) -> usize {
    let mut o = 0usize;
    while o < tampon.len() {
        if tampon[o] == 0 {
            o += 1;
            continue;
        }
        let Some((valeur, lus)) = varint(tampon, o) else {
            break; // varint à cheval sur deux morceaux : on attend
        };
        if valeur <= 3 {
            o += 1;
            continue;
        }
        let taille = valeur - 3;
        if taille > 65535 {
            o += 1;
            continue;
        }
        if o + taille > tampon.len() {
            break; // paquet incomplet : on attend
        }
        let paquet = &tampon[o..o + taille];
        if paquet.len() > lus + 1 && paquet[lus] == 0xFF && paquet[lus + 1] == 0xFF {
            // Un groupe compressé occupe un octet de plus au premier niveau.
            if o + taille + 1 > tampon.len() {
                break;
            }
            let charge = &tampon[o + lus..o + taille + 1];
            if charge.len() > 6 {
                let attendu =
                    u32::from_le_bytes([charge[2], charge[3], charge[4], charge[5]]) as usize;
                if attendu > 0 && attendu <= 1_000_000 {
                    if let Ok(decompresse) = lz4_flex::block::decompress(&charge[6..], attendu) {
                        parcourir_a(&decompresse, &mut |p| observer(p), 1);
                    }
                }
            }
            o += taille + 1;
        } else {
            observer(paquet);
            o += taille;
        }
    }
    o
}

// ---------------------------------------------------------------------------
// Découpage du flux
// ---------------------------------------------------------------------------

/// Entier à longueur variable : (valeur, octets lus).
fn varint(d: &[u8], o: usize) -> Option<(usize, usize)> {
    let mut valeur = 0usize;
    let mut decalage = 0;
    let mut lus = 0;
    loop {
        let octet = *d.get(o + lus)?;
        lus += 1;
        valeur |= ((octet & 0x7F) as usize) << decalage;
        if octet & 0x80 == 0 {
            return Some((valeur, lus));
        }
        decalage += 7;
        if decalage >= 32 {
            return None;
        }
    }
}

/// Le premier endroit du tampon où commence vraiment un paquet.
///
/// Un tampon commence là où le meter a commencé à écouter, donc presque toujours
/// au milieu d'un paquet : ses premiers octets sont la fin du précédent. Parti de
/// là, le découpage se décale, et un décalage ne se rattrape pas toujours : une
/// longueur lue de travers fait sauter par-dessus un groupe compressé entier, et
/// tout son contenu est perdu. C'est ce qui est arrivé le 29/09/2026 — le
/// Combat Power défile en trois paquets et le site n'a reçu que le premier.
///
/// Ce qu'on prend comme preuve d'un bon départ : **une décompression qui
/// réussit**. Le flux du jeu est fait pour l'essentiel de groupes compressés, et
/// un découpage décalé ne produit presque jamais un bloc valide de la taille
/// annoncée — alors qu'un découpage juste les enchaîne tous. Compter les octets
/// « expliqués » ne suffisait pas : plusieurs départs y arrivent aussi bien, et
/// on retenait le mauvais.
///
/// Renvoie `None` si aucune position ne convainc : mieux vaut ne rien relire que
/// relire de travers.
const FENETRE_DE_CONTROLE: usize = 64 * 1024;
const CANDIDATS_AU_PLUS: usize = 4096;
const GROUPES_POUR_ETRE_SUR: usize = 4;
const MINIMUM_CREDIBLE: usize = 32;

/// Ce qu'un départ explique : combien de groupes compressés s'y décompressent, et
/// combien d'octets s'enchaînent sans le moindre rattrapage.
fn qualite(flux: &[u8], debut: usize) -> (usize, usize) {
    let limite = (debut + FENETRE_DE_CONTROLE).min(flux.len());
    let mut o = debut;
    let mut groupes = 0usize;
    while o < limite {
        // Comme le découpeur : un octet nul est du remplissage, on passe.
        if flux[o] == 0 {
            o += 1;
            continue;
        }
        let Some((valeur, lus)) = varint(flux, o) else {
            break;
        };
        if valeur <= 3 {
            break;
        }
        let taille = valeur - 3;
        if taille > 65535 || o + taille > flux.len() {
            break;
        }
        let paquet = &flux[o..o + taille];
        if paquet.len() > lus + 1 && paquet[lus] == 0xFF && paquet[lus + 1] == 0xFF {
            // Un groupe compressé occupe un octet de plus au premier niveau.
            let fin = (o + taille + 1).min(flux.len());
            let charge = &flux[o + lus..fin];
            if charge.len() <= 6 {
                break;
            }
            let attendu =
                u32::from_le_bytes([charge[2], charge[3], charge[4], charge[5]]) as usize;
            if attendu == 0
                || attendu > 1_000_000
                || lz4_flex::block::decompress(&charge[6..], attendu).is_err()
            {
                break; // un groupe qui ne se décompresse pas : ce départ est faux
            }
            groupes += 1;
            if groupes >= GROUPES_POUR_ETRE_SUR {
                return (groupes, limite - debut); // assez vu
            }
            o += taille + 1;
        } else {
            o += taille;
        }
    }
    (groupes, o - debut)
}

pub fn debut_aligne(flux: &[u8]) -> Option<usize> {
    let mut meilleur: Option<(usize, usize, usize)> = None; // (groupes, octets, début)
    for debut in 0..flux.len().min(CANDIDATS_AU_PLUS) {
        let (groupes, octets) = qualite(flux, debut);
        if groupes >= GROUPES_POUR_ETRE_SUR {
            return Some(debut);
        }
        // Un départ qui explique le tampon jusqu'au bout convient aussi : les
        // tampons courts n'ont pas toujours un seul groupe compressé.
        if octets > 0 && debut + octets >= flux.len() && groupes > 0 {
            return Some(debut);
        }
        if meilleur.map_or(true, |(g, o, _)| (groupes, octets) > (g, o)) {
            meilleur = Some((groupes, octets, debut));
        }
    }
    meilleur
        .filter(|(groupes, octets, debut)| {
            *groupes > 0 || (*octets >= MINIMUM_CREDIBLE && debut + octets >= flux.len())
        })
        .map(|(_, _, debut)| debut)
}

/// Même découpage que `consume_stream` d'A2Tools : longueur varint (moins 3) et
/// groupes compressés `FF FF`. Deux détails qui comptent : au premier niveau un
/// groupe occupe **un octet de plus**, et on se resynchronise octet par octet
/// plutôt que d'abandonner le flux à la première anomalie — un tampon commence
/// rarement pile au début d'un paquet.
pub fn parcourir(flux: &[u8], voir: &mut impl FnMut(&[u8])) {
    parcourir_a(flux, voir, 0);
}

fn parcourir_a(flux: &[u8], voir: &mut impl FnMut(&[u8]), profondeur: u32) {
    if profondeur > 4 {
        return;
    }
    let interne = profondeur > 0;
    let mut o = 0usize;
    while o < flux.len() {
        if flux[o] == 0 {
            o += 1;
            continue;
        }
        let Some((valeur, lus)) = varint(flux, o) else {
            if interne {
                break;
            }
            o += 1;
            continue;
        };
        if valeur <= 3 {
            if interne {
                break;
            }
            o += 1;
            continue;
        }
        let taille = valeur - 3;
        if taille > 65535 || o + taille > flux.len() {
            if interne {
                break;
            }
            o += 1;
            continue;
        }

        let paquet = &flux[o..o + taille];
        let est_groupe = paquet.len() > lus + 1 && paquet[lus] == 0xFF && paquet[lus + 1] == 0xFF;

        if est_groupe {
            let supplement = if interne { 0 } else { 1 };
            let fin = (o + taille + supplement).min(flux.len());
            let charge = &flux[o + lus..fin];
            if charge.len() > 6 {
                let taille_decompressee =
                    u32::from_le_bytes([charge[2], charge[3], charge[4], charge[5]]) as usize;
                if taille_decompressee > 0 && taille_decompressee <= 1_000_000 {
                    if let Ok(decompresse) =
                        lz4_flex::block::decompress(&charge[6..], taille_decompressee)
                    {
                        parcourir_a(&decompresse, voir, profondeur + 1);
                    }
                }
            }
            o += taille + supplement;
        } else {
            voir(paquet);
            o += taille;
        }
    }
}
