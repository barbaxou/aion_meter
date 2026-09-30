// Vérification de mise à jour — NEUTRALISÉE par la guilde XIII NRV.
//
// La version d'origine interrogeait https://a2tools.app/latest-v2.json, y lisait
// une adresse de fichier MSI, la téléchargeait dans le dossier temporaire et
// lançait `msiexec /i <fichier> /passive`. Aucune signature vérifiée, aucune
// empreinte comparée, aucune restriction sur le domaine du MSI : qui contrôle ce
// serveur pouvait installer le programme de son choix sur le poste de chaque
// membre. La fenêtre s'annonçait « A2Tools - Update Available », que nos membres
// n'auraient même pas reconnue comme venant de nous.
//
// On distribue nos propres versions ; il n'y a donc rien à vérifier ici. Le
// fichier est conservé, vide, pour que la page qui l'inclut ne casse pas et pour
// que la raison de cette absence reste écrite quelque part.

window.ReleaseChecker = { start() {} };
