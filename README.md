# DesktopFlora

Schnee, Laub, Regen, Kirschblüten oder Sterne auf dem Windows-Desktop: fällt sanft, bleibt auf Fenstern und am Boden liegen, rutscht beim Verschieben ab und lässt sich mit dem Mauszeiger wegfegen.

DesktopFlora basiert auf [Snowhush](https://github.com/mbezruchko/Snowhush) von Mykhailo Bezruchko und übernimmt dessen aktuellen Stand (v0.13.1) mit allen Wettern und Verbesserungen, dazu ein eigenes Einstellungsfenster und eigene Funktionen.

**Download:** [Neuestes Release](https://github.com/clavicular-alt/DesktopFlora/releases/latest) → `DesktopFlora-Setup.exe`. Installiert nur für den aktuellen Benutzer, ohne Adminrechte, und ersetzt eine ältere Installation; die Einstellungen bleiben.

## Was fällt

- **Schnee:** jede Flocke mit eigenem Tempo, Haufen am Boden und auf Fenstern, rutscht beim Ziehen ab, fällt beim Schließen oder Minimieren in Klumpen herunter.
- **Herbstlaub:** Blätter nach echten Fotos, die sich drehen und wenden, sichtbare Laubhaufen aus einzelnen Blättern, „Laub verwelken“.
- **Regen:** prasselt und spritzt, läuft an Fenstern und der Taskleiste herunter wie Tropfen auf einer Scheibe, hängt an Unterkanten, lässt sich mit der Maus verwischen.
- **Kirschblüten:** gewölbte, glänzende Blütenblätter, ab und zu eine ganze Blüte mit Stiel, „Blüten welken“.
- **Sterne:** kleine Lichter mit Strahlen, die langsam fallen und funkeln.
- **Zwei mischen**, zum Beispiel Schnee und Laub.
- **Halloween:** Kürbislaternen im Schnee und Geister mit Schildern, vom 30.10. bis 1.11. oder auf Wunsch.
- **Jahreszeit:** Winter Schnee, Frühling Blüten, Sommer Sterne, Herbst Laub, passend zum Land aus den Windows-Regionseinstellungen (Südhalbkugel umgekehrt).
- **Wetter vor Ort:** Regen oder Schnee wie draußen, mit Stärke und Wind; bei trockenem Wetter die Jahreszeit am Standort (Open-Meteo, Standort von Windows auf etwa 1 km gerundet, alle 30 Minuten).

## Einstellungsfenster

Links- oder Rechtsklick aufs Symbol im Infobereich öffnet ein schlichtes Schwarz-Weiß-Fenster mit Milchglas (Windows 11), hell oder dunkel wie Windows, mit fünf Reitern:

- **Wetter:** woher es kommt (selbst wählen, Jahreszeit, Wetter vor Ort), was fällt, mischen, Halloween, Menge als Schieberegler, Wegfegen.
- **Aussehen:** Größe, Wind, Deckkraft als Schieberegler, natürliche oder bis zu fünf eigene Farben für Schnee und Sterne.
- **Verhalten:** Mit Windows starten, liegen bleiben auf Boden und Fenstern, Schmelzen/Verwelken, Schnee auf einer Desktop-Auswahl, beim Tippen ausblenden, Zeitplan.
- **Anzeige:** hinter Fenstern, über Vollbild, hinter der Taskleiste (z. B. mit TranslucentTB), in Screenshots, Bildrate (Eco 20 fps, Normal, Max).
- **Mehr:** schneefreie Programme, frei wählbare Tastenkürzel, Updates, Snowhush unterstützen.

Doppelklick aufs Symbol schaltet an und aus. Tastenkürzel (änderbar): Strg+Alt+S an/aus, Strg+Alt+C wegfegen, Strg+Alt+W nächstes Wetter.

## Sonst noch

- Spricht Deutsch, Englisch, Spanisch, Französisch, Japanisch, Portugiesisch, Russisch, Ukrainisch und Chinesisch.
- Windows-Benachrichtigungen fegen den Schnee beiseite, ein Fenster nimmt seinen Schnee auf einen anderen Bildschirm mit, der Schnee folgt den Ecken jedes Fensters.
- Pausiert bei Sperrbildschirm, ausgeschaltetem Display und Vollbild-Apps; weniger Last im Akku- und Energiesparbetrieb; Effizienzmodus und Speicherfreigabe, solange nichts zu sehen ist.
- Updates in der App: lädt das neue Setup, prüft es gegen die SHA-256-Prüfsumme des Releases und installiert nur bei Übereinstimmung.
- Netzwerk nur für den Update-Check (`api.github.com`, `github.com`) und, wenn eingeschaltet, das Wetter (`api.open-meteo.com`). Keine Telemetrie, kein Tastatur- oder Maus-Hook.

## Bauen

GitHub Actions baut bei jedem Push auf `main` `DesktopFlora-Setup.exe` und veröffentlicht sie als Release `v<Version>` (aus `windows/Cargo.toml`) mit `SHA256SUMS.txt`. Optional: Signieren mit den Secrets `SIGN_CERT_BASE64` und `SIGN_PASSWORD`, Einreichen bei winget mit `WINGET_TOKEN` (siehe `packaging/winget`).

Lokal unter Windows (Rust stable):

```powershell
cargo test --manifest-path core/Cargo.toml
cargo build --release --manifest-path windows/Cargo.toml --bin DesktopFlora
$env:SNOWHUSH_APP = "$PWD\windows\target\release\DesktopFlora.exe"
cargo build --release --manifest-path windows/Cargo.toml --bin DesktopFlora-Setup --features setup
```

## Lizenz

[PolyForm Noncommercial 1.0.0](LICENSE), wie Snowhush ab Version 0.6.2: nutzen, weitergeben und ändern ist erlaubt, verkaufen oder kommerziell nutzen nicht. Die `Required Notice`-Zeilen in der Lizenz müssen bei jeder Weitergabe erhalten bleiben.
