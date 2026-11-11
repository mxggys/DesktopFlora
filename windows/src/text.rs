//! The menu and messages in German when Windows is set to German, in English otherwise.

use std::sync::OnceLock;

use windows::Win32::Globalization::GetUserDefaultUILanguage;

fn german() -> bool {
    static GERMAN: OnceLock<bool> = OnceLock::new();
    // The primary language is the low ten bits; 0x07 is German.
    *GERMAN.get_or_init(|| unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x07)
}

/// `text` in the person's language. Placeholders `{}` are filled in order by `fill`.
pub fn t(text: &str) -> String {
    if !german() {
        return text.to_string();
    }
    let translated = match text {
        "Snowfall" => "Schneefall",
        "More Weather" => "Mehr Wetter",
        "About" => "Info",
        "Choose it yourself, follow the season where you live, or the weather outside" => "Selbst wählen, der Jahreszeit bei dir folgen oder dem Wetter draußen",
        "Two kinds fall together" => "Zwei Arten fallen gleichzeitig",
        "Jack-o'-lanterns and ghosts in the snow" => "Kürbislaternen und Geister im Schnee",
        "Only over snow alone" => "Nur wenn allein Schnee fällt",
        "As heavy as outside: {}" => "So stark wie draußen: {}",
        "How much falls" => "Wie viel fällt",
        "Blows away everything lying on the screen" => "Weht alles weg, was auf dem Bildschirm liegt",
        "How big flakes and stars are drawn" => "Wie groß Flocken und Sterne sind",
        "How hard the breeze blows" => "Wie stark der Wind weht",
        "How see-through what falls is" => "Wie durchsichtig es ist",
        "Snow and stars take your colors; leaves, petals and rain keep their own" => "Schnee und Sterne nehmen deine Farben; Laub, Blüten und Regen behalten ihre",
        "Starts with Windows" => "Startet mit Windows",
        "{}: {}" => "{}: {}",
        "Drifts along the bottom of the screen" => "Schneewehen am unteren Bildschirmrand",
        "Settles on the tops of windows and slides off when you drag them" => "Bleibt oben auf Fenstern liegen und rutscht beim Ziehen ab",
        "What lies thaws slowly, so it never takes over the screen" => "Was liegt, taut langsam, damit es nie den Bildschirm füllt",
        "Also settles on a selection dragged across the desktop" => "Bleibt auch auf einem Auswahlrechteck auf dem Desktop liegen",
        "Turns faint while you type" => "Wird blass, solange du tippst",
        "When it shows" => "Wann es sichtbar ist",
        "Falls behind your windows, in front of the wallpaper" => "Fällt hinter deinen Fenstern, vor dem Hintergrund",
        "Stays over full-screen apps and games" => "Bleibt über Vollbild-Apps und Spielen",
        "Settles under a see-through taskbar, as with TranslucentTB" => "Legt sich unter eine durchsichtige Taskleiste, z. B. mit TranslucentTB",
        "Screenshots and screen sharing show it too" => "Auch in Screenshots und beim Bildschirm teilen sichtbar",
        "Eco: at most 20 frames a second. Max: always the display's full rate" => "Eco: höchstens 20 Bilder pro Sekunde. Max: immer volle Bildrate",
        "No snow on its windows; maximized in front, none on that screen at all" => "Kein Schnee auf seinen Fenstern; maximiert im Vordergrund gar keiner auf dem Bildschirm",
        "Add" => "Hinzufügen",
        "Add a program" => "Programm hinzufügen",
        "Switch to the program, then open the settings again" => "Zum Programm wechseln, dann die Einstellungen wieder öffnen",
        "No programs yet" => "Noch keine Programme",
        "Snow falls on every window" => "Schnee fällt auf alle Fenster",
        "Snow-free" => "Schneefrei",
        "Remove" => "Entfernen",
        "Click, then press the new keys; Delete removes it" => "Anklicken, dann die neuen Tasten drücken; Entf löscht es",
        "Double-click the tray icon" => "Doppelklick aufs Symbol im Infobereich",
        "Switches the snow on and off" => "Schaltet den Schnee an und aus",
        "Updates install themselves, checked against the release's checksum" => "Updates installieren sich selbst, geprüft gegen die Prüfsumme des Releases",
        "Based on Snowhush" => "Basiert auf Snowhush",
        "By Mykhailo Bezruchko, under the PolyForm Noncommercial license" => "Von Mykhailo Bezruchko, unter der Lizenz PolyForm Noncommercial",
        "On" => "Ein",
        "Off" => "Aus",
        "Settings…" => "Einstellungen…",
        "DesktopFlora is now {}" => "DesktopFlora ist jetzt {}",
        "Updated from {}." => "Aktualisiert von {}.",
        "Snow on" => "Schnee an",
        "Look" => "Aussehen",
        "Size" => "Größe",
        "Color" => "Farbe",
        "Natural" => "Natürlich",
        "Ice Blue" => "Eisblau",
        "Mint" => "Mint",
        "Gold" => "Gold",
        "Pink" => "Rosa",
        "Lavender" => "Lavendel",
        "Red" => "Rot",
        "Orange" => "Orange",
        "Silver" => "Silber",
        "Keep Snow Off {}" => "Kein Schnee auf {}",
        "Switch to a program, then come back here" => "Erst zu einem Programm wechseln, dann hierher",
        "Frame Rate" => "Bildrate",
        "Schedule" => "Zeitplan",
        "Display" => "Anzeige",
        "Mix two" => "Zwei mischen",
        "Leaves" => "Laub",
        "Blossoms" => "Blüten",
        "My Colors" => "Eigene Farben",
        "Snow and stars take your colors; leaves and petals keep their own." => "Schnee und Sterne nehmen deine Farben; Laub und Blüten behalten ihre.",
        "Normal" => "Normal",
        "Max" => "Max",
        "Eco draws at most 20 frames a second; Max always at the display's full rate." => "Eco zeichnet höchstens 20 Bilder pro Sekunde, Max immer in voller Bildrate.",
        "Fade While Typing" => "Beim Tippen ausblenden",
        "Snow-Free Programs" => "Schneefreie Programme",
        "Keyboard Shortcuts" => "Tastenkürzel",
        "Click to install it." => "Zum Installieren klicken.",
        "Choose" => "Selbst wählen",
        "Season" => "Jahreszeit",
        "Outside" => "Wetter vor Ort",
        "Pick colors below" => "Unten Farben auswählen",
        "Click one to remove it" => "Antippen zum Entfernen",
        "Full: remove one to add another" => "Voll: eine entfernen für eine neue",
        "Winter" => "Winter",
        "Spring" => "Frühling",
        "Summer" => "Sommer",
        "Autumn" => "Herbst",
        "Dry outside: {} for the {}" => "Draußen trocken: {} zum {}",
        "Place: your location from Windows, rounded to about 1 km" => "Ort: dein Windows-Standort, auf etwa 1 km gerundet",
        "your region" => "deiner Region",
        "Season in {}, from the region set in Windows" => "Jahreszeit in {}, aus der Region in Windows",
        "{} · {}" => "{} · {}",
        "Paused" => "Pausiert",
        "Behavior" => "Verhalten",
        "Overview" => "Übersicht",
        "Programs & Shortcuts" => "Programme & Tastenkürzel",
        "More" => "Mehr",
        "What falls" => "Was fällt",
        "Amount" => "Menge",
        "Stars" => "Sterne",
        "+ Keep Snow Off {}" => "+ Kein Schnee auf {}",
        "Switch to a program, then open this again" => "Erst zu einem Programm wechseln, dann hier öffnen",
        "and {} more" => "und {} weitere",
        "Snow on/off" => "An / Aus",
        "Clear snow" => "Wegfegen",
        "Next weather" => "Nächstes Wetter",
        "Press keys…" => "Tasten drücken…",
        "{} (taken)" => "{} (belegt)",
        "None" => "Keins",
        "Click a shortcut, then press the new keys. Delete removes it." => "Kürzel anklicken, dann neue Tasten drücken. Entf löscht es.",
        "Ctrl" => "Strg",
        "Shift" => "Umschalt",
        "Outside right now: {}" => "Draußen gerade: {}",
        "Asking for the weather outside…" => "Wetter wird abgefragt…",
        "Outside the schedule right now" => "Gerade außerhalb des Zeitplans",
        "Windows didn't share the location. Turn on Location and \"Let desktop apps access your location\" in Settings → Privacy & security → Location." => {
            "Windows hat den Standort nicht freigegeben. In Einstellungen → Datenschutz und Sicherheit → Position die Ortung und „Desktop-Apps den Zugriff auf Ihren Standort erlauben“ einschalten."
        }
        "Light" => "Leicht",
        "Steady" => "Mittel",
        "Heavy" => "Stark",
        "Variable" => "Wechselnd",
        "Weather" => "Wetter",
        "Snow" => "Schnee",
        "Rain" => "Regen",
        "Small" => "Klein",
        "Large" => "Groß",
        "Wind" => "Wind",
        "Still" => "Windstill",
        "Breezy" => "Brise",
        "Stormy" => "Stürmisch",
        "Mouse Wind" => "Wind von der Maus",
        "Gentle" => "Sanft",
        "Strong" => "Stark",
        "Glitter" => "Glitzern",
        "Sparkly" => "Viel",
        "Opacity" => "Deckkraft",
        "Eco" => "Sparsam",
        "Always" => "Immer",
        "Evenings" => "Abends",
        "Daytime" => "Tagsüber",
        "Nights" => "Nachts",
        "Accumulate" => "Liegen bleiben",
        "Melt Snow" => "Schnee schmelzen",
        "Snow on Windows" => "Schnee auf Fenstern",
        "Clear Snow" => "Schnee wegfegen",
        "Behind Windows" => "Hinter Fenstern",
        "Show in Screenshots" => "In Screenshots zeigen",
        "Open at Login" => "Mit Windows starten",
        "Check for Updates…" => "Nach Updates suchen…",
        "Quit" => "Beenden",
        "Couldn't get the local weather.\n\n{}" => "Das Wetter vor Ort ließ sich nicht abrufen.\n\n{}",
        "DesktopFlora {} is available" => "DesktopFlora {} ist verfügbar",
        "You're up to date.\n\nDesktopFlora {} is the latest version." => "Alles aktuell.\n\nDesktopFlora {} ist die neueste Version.",
        "Couldn't check for updates.\n\n{}" => "Die Suche nach Updates hat nicht geklappt.\n\n{}",
        "DesktopFlora {} is available. You have {}.\n\nOpen the download page?" => {
            "DesktopFlora {} ist verfügbar. Installiert ist {}.\n\nDownload-Seite öffnen?"
        }
        "DesktopFlora {} is available. You have {}.\n\nInstall it now? DesktopFlora downloads it, checks it against the release's checksum and restarts." => {
            "DesktopFlora {} ist verfügbar. Installiert ist {}.\n\nJetzt installieren? DesktopFlora lädt sie herunter, prüft die Prüfsumme und startet neu."
        }
        "Couldn't install the update.\n\n{}\n\nOpen the download page instead?" => {
            "Das Update ließ sich nicht installieren.\n\n{}\n\nStattdessen die Download-Seite öffnen?"
        }
        _ => text,
    };
    translated.to_string()
}

/// Fills the `{}` placeholders of a translated text in order.
pub fn fill(text: &str, values: &[&str]) -> String {
    let mut result = String::new();
    let mut parts = text.split("{}");
    if let Some(first) = parts.next() {
        result.push_str(first);
    }
    for (i, part) in parts.enumerate() {
        result.push_str(values.get(i).copied().unwrap_or(""));
        result.push_str(part);
    }
    result
}
