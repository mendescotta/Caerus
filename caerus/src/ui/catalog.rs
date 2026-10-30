//! A curated application index; names resolve only against the active backend.
//! Void candidates and desktop IDs can be checked with scripts/check-discover-void.py.
use crate::backend::package::Package;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub struct CatalogApp {
    pub category: &'static str,
    pub name: &'static str,
    pub packages: &'static [&'static str],
    /// Verified launcher filename, or empty when the package ships none.
    pub desktop: &'static str,
}

pub const CATEGORIES: &[&str] = &[
    "Web",
    "Mail & communication",
    "News & podcasts",
    "Office & documents",
    "Notes & planning",
    "Finance",
    "Editors",
    "Graphics",
    "Photography",
    "Sound",
    "Video",
    "Games",
    "Emulation",
    "Education",
    "Science & mathematics",
    "CAD & engineering",
    "Development",
    "Reverse engineering",
    "Virtualization & containers",
    "Network",
    "Backup & recovery",
    "Security & privacy",
    "Files & storage",
    "Terminals",
    "Accessibility",
    "Desktop & appearance",
    "Printing & scanning",
    "Utilities",
];

/// Front-page recommendations, by catalog name, shown in this order when available.
pub const FEATURED: &[&str] = &[
    "Firefox",
    "Thunderbird",
    "LibreOffice",
    "GIMP",
    "Inkscape",
    "Krita",
    "VLC",
    "OBS Studio",
    "Kdenlive",
    "Audacity",
    "Blender",
    "KeepassXC",
    "Xournal++",
    "Stellarium",
    "Syncthing",
    "SuperTuxKart",
    "Transmission",
    "Geany",
];

/// Symbolic icon and accent style class for a section tile.
pub fn category_style(category: &str) -> (&'static str, &'static str) {
    match category {
        "Web" => ("web-browser-symbolic", "accent-blue"),
        "Mail & communication" => ("mail-send-symbolic", "accent-teal"),
        "News & podcasts" => ("application-rss+xml-symbolic", "accent-orange"),
        "Office & documents" => ("x-office-document-symbolic", "accent-orange"),
        "Notes & planning" => ("x-office-calendar-symbolic", "accent-yellow"),
        "Finance" => ("accessories-calculator-symbolic", "accent-green"),
        "Editors" => ("text-editor-symbolic", "accent-slate"),
        "Graphics" => ("image-x-generic-symbolic", "accent-pink"),
        "Photography" => ("camera-photo-symbolic", "accent-purple"),
        "Sound" => ("audio-x-generic-symbolic", "accent-green"),
        "Video" => ("video-x-generic-symbolic", "accent-red"),
        "Games" => ("applications-games-symbolic", "accent-purple"),
        "Education" => ("accessories-dictionary-symbolic", "accent-yellow"),
        "Science & mathematics" => ("applications-science-symbolic", "accent-teal"),
        "CAD & engineering" => ("x-office-drawing-symbolic", "accent-slate"),
        "Development" => ("applications-engineering-symbolic", "accent-blue"),
        "Network" => ("network-workgroup-symbolic", "accent-green"),
        "Reverse engineering" => ("applications-engineering-symbolic", "accent-purple"),
        "Virtualization & containers" => ("computer-symbolic", "accent-blue"),
        "Backup & recovery" => ("document-save-symbolic", "accent-teal"),
        "Security & privacy" => ("security-high-symbolic", "accent-red"),
        "Files & storage" => ("drive-harddisk-symbolic", "accent-orange"),
        "Terminals" => ("utilities-terminal-symbolic", "accent-slate"),
        "Accessibility" => ("preferences-desktop-accessibility-symbolic", "accent-green"),
        "Emulation" => ("input-gaming-symbolic", "accent-red"),
        "Desktop & appearance" => ("preferences-desktop-appearance-symbolic", "accent-pink"),
        "Printing & scanning" => ("printer-symbolic", "accent-slate"),
        _ => ("applications-utilities-symbolic", "accent-slate"),
    }
}

macro_rules! app {
    ($category:literal, $name:literal, $desktop:literal, $($package:literal),+) => {
        CatalogApp { category: $category, name: $name, desktop: $desktop, packages: &[$($package),+] }
    };
}

pub const CATALOG: &[CatalogApp] = &[
    app!("Web", "Firefox", "firefox.desktop", "firefox"),
    app!("Web", "Chromium", "chromium.desktop", "chromium"),
    app!("Web", "Falkon", "falkon.desktop", "falkon"),
    app!("Web", "GNOME Web", "epiphany.desktop", "epiphany"),
    app!("Web", "Dillo", "dillo.desktop", "dillo"),
    app!("Web", "qutebrowser", "qutebrowser.desktop", "qutebrowser"),
    app!("Web", "Links", "links.desktop", "links"),
    app!("Web", "w3m", "w3m.desktop", "w3m"),
    app!("Web", "aria2", "aria2.desktop", "aria2"),
    app!("Web", "wget", "wget.desktop", "wget"),
    app!(
        "Mail & communication",
        "Thunderbird",
        "thunderbird.desktop",
        "thunderbird"
    ),
    app!(
        "Mail & communication",
        "Claws Mail",
        "claws-mail.desktop",
        "claws-mail"
    ),
    app!("Mail & communication", "Geary", "geary.desktop", "geary"),
    app!(
        "Mail & communication",
        "Evolution",
        "evolution.desktop",
        "evolution"
    ),
    app!("Mail & communication", "Mutt", "mutt.desktop", "mutt"),
    app!(
        "Mail & communication",
        "Neomutt",
        "neomutt.desktop",
        "neomutt"
    ),
    app!("Mail & communication", "Pidgin", "pidgin.desktop", "pidgin"),
    app!(
        "Mail & communication",
        "HexChat",
        "hexchat.desktop",
        "hexchat"
    ),
    app!("Mail & communication", "Dino", "dino.desktop", "dino"),
    app!(
        "Mail & communication",
        "Polari",
        "org.gnome.Polari.desktop",
        "polari"
    ),
    app!(
        "Mail & communication",
        "Sylpheed",
        "sylpheed.desktop",
        "sylpheed"
    ),
    app!("Mail & communication", "Gajim", "gajim.desktop", "gajim"),
    app!(
        "Mail & communication",
        "WeeChat",
        "weechat.desktop",
        "weechat"
    ),
    app!(
        "Mail & communication",
        "Telegram",
        "telegram-desktop.desktop",
        "telegram-desktop"
    ),
    app!(
        "Mail & communication",
        "Signal",
        "Signal-Desktop.desktop",
        "Signal-Desktop"
    ),
    app!(
        "Office & documents",
        "LibreOffice",
        "libreoffice-writer.desktop",
        "libreoffice",
        "libreoffice-writer"
    ),
    app!(
        "Office & documents",
        "AbiWord",
        "abiword.desktop",
        "abiword"
    ),
    app!(
        "Office & documents",
        "Gnumeric",
        "gnumeric.desktop",
        "gnumeric"
    ),
    app!("Office & documents", "Evince", "evince.desktop", "evince"),
    app!(
        "Notes & planning",
        "Gnote",
        "org.gnome.Gnote.desktop",
        "gnote"
    ),
    app!(
        "Office & documents",
        "ghostwriter",
        "org.kde.ghostwriter.desktop",
        "ghostwriter"
    ),
    app!("Office & documents", "Okular", "okular.desktop", "okular"),
    app!(
        "Office & documents",
        "Zathura",
        "zathura.desktop",
        "zathura"
    ),
    app!(
        "Office & documents",
        "Foliate",
        "foliate.desktop",
        "foliate"
    ),
    app!(
        "Office & documents",
        "Calibre",
        "calibre.desktop",
        "calibre"
    ),
    app!(
        "Office & documents",
        "Scribus",
        "scribus.desktop",
        "scribus"
    ),
    app!(
        "Office & documents",
        "PDF Arranger",
        "pdfarranger.desktop",
        "pdfarranger"
    ),
    app!(
        "Office & documents",
        "Xournal++",
        "xournalpp.desktop",
        "xournalpp"
    ),
    app!(
        "Office & documents",
        "TeXstudio",
        "texstudio.desktop",
        "texstudio"
    ),
    app!("Editors", "Geany", "geany.desktop", "geany"),
    app!(
        "Editors",
        "GNOME Text Editor",
        "org.gnome.TextEditor.desktop",
        "gnome-text-editor"
    ),
    app!("Editors", "Gedit", "gedit.desktop", "gedit"),
    app!("Editors", "Kate", "kate.desktop", "kate"),
    app!("Editors", "KWrite", "kwrite.desktop", "kwrite"),
    app!("Editors", "Mousepad", "mousepad.desktop", "mousepad"),
    app!("Editors", "Leafpad", "leafpad.desktop", "leafpad"),
    app!("Editors", "Neovim", "neovim.desktop", "neovim"),
    app!("Editors", "Vim", "vim.desktop", "vim"),
    app!("Editors", "Emacs", "emacs.desktop", "emacs", "emacs-gtk3"),
    app!("Editors", "Helix", "helix.desktop", "helix"),
    app!("Editors", "Nano", "nano.desktop", "nano"),
    app!("Editors", "Micro", "micro.desktop", "micro"),
    app!("Graphics", "GIMP", "gimp.desktop", "gimp"),
    app!(
        "Graphics",
        "Inkscape",
        "org.inkscape.Inkscape.desktop",
        "inkscape"
    ),
    app!("Graphics", "Krita", "krita.desktop", "krita"),
    app!("Graphics", "Blender", "blender.desktop", "blender"),
    app!("Photography", "Darktable", "darktable.desktop", "darktable"),
    app!(
        "Photography",
        "RawTherapee",
        "rawtherapee.desktop",
        "rawtherapee"
    ),
    app!("Graphics", "MyPaint", "mypaint.desktop", "mypaint"),
    app!("Graphics", "Pinta", "pinta.desktop", "pinta"),
    app!("Photography", "gThumb", "gthumb.desktop", "gthumb"),
    app!("Graphics", "Gwenview", "gwenview.desktop", "gwenview"),
    app!("Photography", "Shotwell", "shotwell.desktop", "shotwell"),
    app!(
        "Graphics",
        "ImageMagick",
        "ImageMagick.desktop",
        "ImageMagick"
    ),
    app!("Graphics", "Viewnior", "viewnior.desktop", "viewnior"),
    app!("Graphics", "Ristretto", "ristretto.desktop", "ristretto"),
    app!(
        "Graphics",
        "nomacs",
        "org.nomacs.ImageLounge.desktop",
        "nomacs"
    ),
    app!("Graphics", "Loupe", "org.gnome.Loupe.desktop", "loupe"),
    app!("Sound", "Audacity", "audacity.desktop", "audacity"),
    app!("Sound", "Ardour", "ardour.desktop", "ardour"),
    app!("Sound", "LMMS", "lmms.desktop", "lmms"),
    app!("Sound", "Qtractor", "qtractor.desktop", "qtractor"),
    app!("Sound", "MuseScore", "musescore.desktop", "musescore"),
    app!("Sound", "Rosegarden", "rosegarden.desktop", "rosegarden"),
    app!("Sound", "Rhythmbox", "rhythmbox.desktop", "rhythmbox"),
    app!("Sound", "Strawberry", "strawberry.desktop", "strawberry"),
    app!("Sound", "Clementine", "clementine.desktop", "clementine"),
    app!("Sound", "Quod Libet", "quodlibet.desktop", "quodlibet"),
    app!(
        "Sound",
        "Easy Effects",
        "easyeffects.desktop",
        "easyeffects"
    ),
    app!(
        "Sound",
        "PulseAudio Volume Control",
        "pavucontrol.desktop",
        "pavucontrol"
    ),
    app!(
        "Sound",
        "SoundConverter",
        "soundconverter.desktop",
        "soundconverter"
    ),
    app!("Sound", "Mixxx", "org.mixxx.Mixxx.desktop", "mixxx"),
    app!(
        "Sound",
        "QjackCtl",
        "org.rncbc.qjackctl.desktop",
        "qjackctl"
    ),
    app!("Video", "VLC", "vlc.desktop", "vlc"),
    app!("Video", "mpv", "mpv.desktop", "mpv"),
    app!("Video", "Celluloid", "celluloid.desktop", "celluloid"),
    app!("Video", "Parole", "parole.desktop", "parole"),
    app!("Video", "SMPlayer", "smplayer.desktop", "smplayer"),
    app!("Video", "Kdenlive", "kdenlive.desktop", "kdenlive"),
    app!("Video", "Shotcut", "shotcut.desktop", "shotcut"),
    app!("Video", "OpenShot", "openshot.desktop", "openshot"),
    app!("Video", "Pitivi", "pitivi.desktop", "pitivi"),
    app!(
        "Video",
        "OBS Studio",
        "com.obsproject.Studio.desktop",
        "obs"
    ),
    app!(
        "Video",
        "HandBrake",
        "fr.handbrake.ghb.desktop",
        "handbrake",
        "HandBrake"
    ),
    app!("Video", "FFmpeg", "ffmpeg.desktop", "ffmpeg"),
    app!("Video", "yt-dlp", "yt-dlp.desktop", "yt-dlp"),
    app!("Video", "Kodi", "org.xbmc.kodi.desktop", "kodi"),
    app!("Video", "Kamoso", "org.kde.kamoso.desktop", "kamoso"),
    app!(
        "Games",
        "SuperTuxKart",
        "supertuxkart.desktop",
        "supertuxkart"
    ),
    app!(
        "Games",
        "SuperTux",
        "supertux2.desktop",
        "supertux2",
        "supertux"
    ),
    app!("Games", "OpenTTD", "openttd.desktop", "openttd"),
    app!(
        "Games",
        "The Battle for Wesnoth",
        "wesnoth.desktop",
        "wesnoth"
    ),
    app!("Games", "0 A.D.", "0ad.desktop", "0ad"),
    app!("Games", "Luanti", "luanti.desktop", "luanti", "minetest"),
    app!("Games", "Neverball", "neverball.desktop", "neverball"),
    app!("Games", "Hedgewars", "hedgewars.desktop", "hedgewars"),
    app!(
        "Games",
        "Warzone 2100",
        "warzone2100.desktop",
        "warzone2100"
    ),
    app!("Games", "GNOME Chess", "gnome-chess.desktop", "gnome-chess"),
    app!(
        "Games",
        "Five or More",
        "org.gnome.five-or-more.desktop",
        "five-or-more"
    ),
    app!(
        "Games",
        "Four-in-a-row",
        "org.gnome.Four-in-a-row.desktop",
        "four-in-a-row"
    ),
    app!(
        "Games",
        "GNOME Mines",
        "org.gnome.Mines.desktop",
        "gnome-mines"
    ),
    app!(
        "Games",
        "GNOME Mahjongg",
        "org.gnome.Mahjongg.desktop",
        "gnome-mahjongg"
    ),
    app!(
        "Games",
        "GNOME Tetravex",
        "org.gnome.Tetravex.desktop",
        "gnome-tetravex"
    ),
    app!(
        "Games",
        "Quadrapassel",
        "org.gnome.Quadrapassel.desktop",
        "quadrapassel"
    ),
    app!(
        "Games",
        "GNOME Sudoku",
        "gnome-sudoku.desktop",
        "gnome-sudoku"
    ),
    app!("Games", "Aisleriot", "aisleriot.desktop", "aisleriot"),
    app!("Games", "Lutris", "lutris.desktop", "lutris"),
    app!("Games", "Steam", "steam.desktop", "steam"),
    app!(
        "Education",
        "GCompris",
        "gcompris-qt.desktop",
        "gcompris-qt"
    ),
    app!("Education", "Tux Paint", "tuxpaint.desktop", "tuxpaint"),
    app!("Education", "Tux Math", "tuxmath.desktop", "tuxmath"),
    app!("Education", "Anki", "anki.desktop", "anki"),
    app!("Education", "KTouch", "ktouch.desktop", "ktouch"),
    app!(
        "Education",
        "KGeography",
        "kgeography.desktop",
        "kgeography"
    ),
    app!("Education", "Kalzium", "kalzium.desktop", "kalzium"),
    app!("Education", "KTurtle", "kturtle.desktop", "kturtle"),
    app!("Education", "Parley", "parley.desktop", "parley"),
    app!("Education", "Marble", "org.kde.marble.desktop", "marble"),
    app!(
        "Science & mathematics",
        "Stellarium",
        "stellarium.desktop",
        "stellarium"
    ),
    app!(
        "Science & mathematics",
        "Celestia",
        "",
        "celestia-glut",
        "celestia"
    ),
    app!(
        "Science & mathematics",
        "KStars",
        "kstars.desktop",
        "kstars"
    ),
    app!(
        "Science & mathematics",
        "GeoGebra",
        "geogebra.desktop",
        "geogebra"
    ),
    app!(
        "Science & mathematics",
        "GNU Octave",
        "octave.desktop",
        "octave"
    ),
    app!("Science & mathematics", "R", "R.desktop", "R"),
    app!(
        "Science & mathematics",
        "Maxima",
        "maxima.desktop",
        "maxima"
    ),
    app!(
        "Science & mathematics",
        "wxMaxima",
        "io.github.wxmaxima_developers.wxMaxima.desktop",
        "wxMaxima",
        "wxmaxima"
    ),
    app!(
        "Science & mathematics",
        "Gnuplot",
        "gnuplot.desktop",
        "gnuplot"
    ),
    app!(
        "Science & mathematics",
        "Qalculate!",
        "qalculate-gtk.desktop",
        "qalculate-gtk"
    ),
    app!(
        "Science & mathematics",
        "SpeedCrunch",
        "speedcrunch.desktop",
        "speedcrunch"
    ),
    app!("CAD & engineering", "KiCad", "kicad.desktop", "kicad"),
    app!(
        "CAD & engineering",
        "FreeCAD",
        "org.freecad.FreeCAD.desktop",
        "freecad",
        "FreeCAD"
    ),
    app!("Science & mathematics", "QGIS", "qgis.desktop", "qgis"),
    app!(
        "Science & mathematics",
        "LabPlot",
        "org.kde.labplot.desktop",
        "labplot"
    ),
    app!(
        "Development",
        "GNOME Builder",
        "org.gnome.Builder.desktop",
        "gnome-builder"
    ),
    app!(
        "Development",
        "Devhelp",
        "org.gnome.Devhelp.desktop",
        "devhelp"
    ),
    app!(
        "Reverse engineering",
        "GNOME Hex Editor",
        "org.gnome.GHex.desktop",
        "ghex"
    ),
    app!(
        "Development",
        "Qt Creator",
        "qtcreator.desktop",
        "qtcreator"
    ),
    app!(
        "Development",
        "Code::Blocks",
        "codeblocks.desktop",
        "codeblocks"
    ),
    app!(
        "Development",
        "Code - OSS",
        "code-oss.desktop",
        "vscode",
        "code-oss"
    ),
    app!("Development", "VSCodium", "vscodium.desktop", "vscodium"),
    app!("Development", "GCC", "gcc.desktop", "gcc"),
    app!("Development", "Clang", "clang.desktop", "clang"),
    app!("Development", "Rust", "rust.desktop", "rust"),
    app!("Development", "Go", "go.desktop", "go"),
    app!("Development", "Python", "python3.desktop", "python3"),
    app!("Development", "CMake", "cmake.desktop", "cmake"),
    app!("Development", "Meson", "meson.desktop", "meson"),
    app!("Development", "GDB", "gdb.desktop", "gdb"),
    app!("Development", "Meld", "meld.desktop", "meld"),
    app!("Development", "Gitg", "gitg.desktop", "gitg"),
    app!("Development", "Git Cola", "git-cola.desktop", "git-cola"),
    app!(
        "Development",
        "SQLite Browser",
        "sqlitebrowser.desktop",
        "sqlitebrowser"
    ),
    app!(
        "Development",
        "KDevelop",
        "org.kde.kdevelop.desktop",
        "kdevelop"
    ),
    app!(
        "Network",
        "Transmission",
        "transmission-gtk.desktop",
        "transmission-gtk",
        "transmission-qt"
    ),
    app!(
        "Network",
        "qBittorrent",
        "qbittorrent.desktop",
        "qbittorrent"
    ),
    app!("Network", "Deluge", "deluge.desktop", "deluge"),
    app!("Network", "FileZilla", "filezilla.desktop", "filezilla"),
    app!("Network", "Remmina", "remmina.desktop", "remmina"),
    app!("Network", "TigerVNC", "tigervnc.desktop", "tigervnc"),
    app!("Network", "Wireshark", "wireshark.desktop", "wireshark"),
    app!("Network", "Nmap", "nmap.desktop", "nmap"),
    app!("Network", "Syncthing", "syncthing.desktop", "syncthing"),
    app!("Network", "Warpinator", "warpinator.desktop", "warpinator"),
    app!("Network", "Nicotine+", "nicotine+.desktop", "nicotine+"),
    app!("Network", "OpenSSH", "openssh.desktop", "openssh"),
    app!(
        "Network",
        "KDE Connect",
        "org.kde.kdeconnect.app.desktop",
        "kdeconnect"
    ),
    app!("Network", "KRDC", "org.kde.krdc.desktop", "krdc"),
    app!(
        "Network",
        "KTorrent",
        "org.kde.ktorrent.desktop",
        "ktorrent"
    ),
    app!(
        "Files & storage",
        "GNOME Disks",
        "gnome-disk-utility.desktop",
        "gnome-disk-utility"
    ),
    app!(
        "Utilities",
        "GNOME Calculator",
        "org.gnome.Calculator.desktop",
        "gnome-calculator"
    ),
    app!(
        "Notes & planning",
        "GNOME Calendar",
        "org.gnome.Calendar.desktop",
        "gnome-calendar"
    ),
    app!(
        "Network",
        "GNOME Maps",
        "org.gnome.Maps.desktop",
        "gnome-maps"
    ),
    app!(
        "Utilities",
        "GNOME Clocks",
        "org.gnome.clocks.desktop",
        "gnome-clocks"
    ),
    app!(
        "Utilities",
        "GNOME Characters",
        "org.gnome.Characters.desktop",
        "gnome-characters"
    ),
    app!(
        "Network",
        "GNOME Connections",
        "org.gnome.Connections.desktop",
        "gnome-connections"
    ),
    app!(
        "Terminals",
        "GNOME Console",
        "org.gnome.Console.desktop",
        "gnome-console"
    ),
    app!(
        "Printing & scanning",
        "Document Scanner",
        "simple-scan.desktop",
        "simple-scan"
    ),
    app!(
        "Utilities",
        "Snapshot",
        "org.gnome.Snapshot.desktop",
        "snapshot"
    ),
    app!(
        "Utilities",
        "Dconf Editor",
        "ca.desrt.dconf-editor.desktop",
        "dconf-editor"
    ),
    app!("Files & storage", "GParted", "gparted.desktop", "gparted"),
    app!("Files & storage", "Baobab", "baobab.desktop", "baobab"),
    app!(
        "Files & storage",
        "File Roller",
        "file-roller.desktop",
        "file-roller"
    ),
    app!(
        "Files & storage",
        "Engrampa",
        "engrampa.desktop",
        "engrampa"
    ),
    app!(
        "Security & privacy",
        "KeepassXC",
        "keepassxc.desktop",
        "keepassxc"
    ),
    app!(
        "Security & privacy",
        "KeePass",
        "keepass.desktop",
        "keepass"
    ),
    app!(
        "Backup & recovery",
        "Déjà Dup",
        "org.gnome.DejaDup.desktop",
        "deja-dup"
    ),
    app!(
        "Backup & recovery",
        "Back In Time",
        "backintime-qt.desktop",
        "backintime-qt",
        "backintime"
    ),
    app!(
        "Utilities",
        "GNOME System Monitor",
        "gnome-system-monitor.desktop",
        "gnome-system-monitor"
    ),
    app!("Utilities", "btop", "btop.desktop", "btop"),
    app!("Utilities", "htop", "htop.desktop", "htop"),
    app!("Utilities", "Fastfetch", "fastfetch.desktop", "fastfetch"),
    app!("Utilities", "Flameshot", "flameshot.desktop", "flameshot"),
    app!("Utilities", "Spectacle", "spectacle.desktop", "spectacle"),
    app!("Files & storage", "Thunar", "Thunar.desktop", "Thunar"),
    app!(
        "Files & storage",
        "Nautilus",
        "nautilus.desktop",
        "nautilus"
    ),
    app!("Files & storage", "Dolphin", "dolphin.desktop", "dolphin"),
    app!("Files & storage", "PCManFM", "pcmanfm.desktop", "pcmanfm"),
    app!("Terminals", "Tilix", "tilix.desktop", "tilix"),
    app!("Utilities", "Tellico", "org.kde.tellico.desktop", "tellico"),
    app!("Files & storage", "Ark", "org.kde.ark.desktop", "ark"),
    app!(
        "Mail & communication",
        "KMail",
        "org.kde.kmail2.desktop",
        "kmail"
    ),
    app!(
        "Mail & communication",
        "Element",
        "element-desktop.desktop",
        "element-desktop"
    ),
    app!("Mail & communication", "Nheko", "nheko.desktop", "nheko"),
    app!(
        "Mail & communication",
        "Quassel IRC",
        "quassel.desktop",
        "quassel"
    ),
    app!(
        "News & podcasts",
        "Liferea",
        "net.sourceforge.liferea.desktop",
        "liferea"
    ),
    app!(
        "News & podcasts",
        "Akregator",
        "org.kde.akregator.desktop",
        "akregator"
    ),
    app!(
        "News & podcasts",
        "RSS Guard",
        "io.github.martinrotter.rssguardlite.desktop",
        "rssguard"
    ),
    app!("Finance", "GnuCash", "gnucash.desktop", "gnucash"),
    app!("Finance", "HomeBank", "homebank.desktop", "homebank"),
    app!(
        "Finance",
        "KMyMoney",
        "org.kde.kmymoney.desktop",
        "kmymoney"
    ),
    app!(
        "Office & documents",
        "KAddressBook",
        "org.kde.kaddressbook.desktop",
        "kaddressbook"
    ),
    app!(
        "Notes & planning",
        "KOrganizer",
        "org.kde.korganizer.desktop",
        "korganizer"
    ),
    app!(
        "Office & documents",
        "Bookworm",
        "com.github.babluboy.bookworm.desktop",
        "bookworm"
    ),
    app!(
        "Office & documents",
        "FocusWriter",
        "focuswriter.desktop",
        "focuswriter"
    ),
    app!(
        "Notes & planning",
        "CherryTree",
        "cherrytree.desktop",
        "cherrytree"
    ),
    app!("Notes & planning", "Zim", "org.zim_wiki.Zim.desktop", "zim"),
    app!(
        "Notes & planning",
        "QOwnNotes",
        "PBE.QOwnNotes.desktop",
        "qownnotes"
    ),
    app!(
        "Photography",
        "digiKam",
        "org.kde.digikam.desktop",
        "digikam"
    ),
    app!("Photography", "Hugin", "hugin.desktop", "hugin"),
    app!(
        "Sound",
        "Hydrogen",
        "org.hydrogenmusic.Hydrogen.desktop",
        "hydrogen"
    ),
    app!("Sound", "Helvum", "org.pipewire.Helvum.desktop", "helvum"),
    app!(
        "Sound",
        "qpwgraph",
        "org.rncbc.qpwgraph.desktop",
        "qpwgraph"
    ),
    app!("News & podcasts", "gPodder", "gpodder.desktop", "gpodder"),
    app!(
        "Sound",
        "Shortwave",
        "de.haeckerfelix.Shortwave.desktop",
        "shortwave"
    ),
    app!(
        "Sound",
        "Lollypop",
        "org.gnome.Lollypop.desktop",
        "lollypop"
    ),
    app!(
        "Sound",
        "Audacious",
        "audacious.desktop",
        "audacious",
        "audacious-base"
    ),
    app!("Sound", "DeaDBeeF", "deadbeef.desktop", "deadbeef"),
    app!("Sound", "Kid3", "org.kde.kid3-qt.desktop", "kid3"),
    app!(
        "Sound",
        "MusicBrainz Picard",
        "org.musicbrainz.Picard.desktop",
        "picard"
    ),
    app!(
        "Sound",
        "Sound Juicer",
        "org.gnome.SoundJuicer.desktop",
        "sound-juicer"
    ),
    app!("Sound", "Asunder", "asunder.desktop", "asunder"),
    app!(
        "CAD & engineering",
        "OpenSCAD",
        "openscad.desktop",
        "openscad"
    ),
    app!(
        "CAD & engineering",
        "SolveSpace",
        "solvespace.desktop",
        "solvespace"
    ),
    app!(
        "Network",
        "Syncthing Tray",
        "syncthingtray.desktop",
        "syncthingtray"
    ),
    app!(
        "Backup & recovery",
        "Pika Backup",
        "org.gnome.World.PikaBackup.desktop",
        "pika-backup"
    ),
    app!(
        "Security & privacy",
        "Passwords and Keys",
        "org.gnome.seahorse.Application.desktop",
        "seahorse"
    ),
    app!(
        "Security & privacy",
        "GNU Privacy Assistant",
        "gpa.desktop",
        "gpa"
    ),
    app!(
        "Security & privacy",
        "Kleopatra",
        "org.kde.kleopatra.desktop",
        "kleopatra"
    ),
    app!(
        "Files & storage",
        "Filelight",
        "org.kde.filelight.desktop",
        "filelight"
    ),
    app!(
        "Files & storage",
        "QDirStat",
        "qdirstat.desktop",
        "qdirstat"
    ),
    app!(
        "Files & storage",
        "KRename",
        "org.kde.krename.desktop",
        "krename"
    ),
    app!("Files & storage", "KFind", "org.kde.kfind.desktop", "kfind"),
    app!("Files & storage", "Brasero", "brasero.desktop", "brasero"),
    app!("Files & storage", "Xfburn", "xfburn.desktop", "xfburn"),
    // Added from the official binary repository audit; see scripts/discover/README.md.
    app!("Backup & recovery", "BorgBackup", "", "borg"),
    app!("Backup & recovery", "borgmatic", "", "borgmatic"),
    app!("Backup & recovery", "Restic", "", "restic"),
    app!("Backup & recovery", "autorestic", "", "autorestic"),
    app!("Backup & recovery", "Kopia", "", "kopia"),
    app!("Backup & recovery", "duplicity", "", "duplicity"),
    app!("Backup & recovery", "rdiff-backup", "", "rdiff-backup"),
    app!("Backup & recovery", "rsnapshot", "", "rsnapshot"),
    app!("Backup & recovery", "btrbk", "", "btrbk"),
    app!("Backup & recovery", "bup", "", "bup"),
    app!(
        "Backup & recovery",
        "Timeshift",
        "timeshift-gtk.desktop",
        "timeshift"
    ),
    app!("Backup & recovery", "Snapper", "", "snapper"),
    app!("Backup & recovery", "GNU ddrescue", "", "ddrescue"),
    app!("Backup & recovery", "TestDisk", "", "testdisk"),
    app!("Backup & recovery", "FSArchiver", "", "fsarchiver"),
    app!("Backup & recovery", "Kup", "kcm_kup.desktop", "kup"),
    app!(
        "Backup & recovery",
        "Qt FSArchiver",
        "qt-fsarchiver.desktop",
        "qt5-fsarchiver"
    ),
    app!("Security & privacy", "age", "", "age"),
    app!("Security & privacy", "GnuPG", "", "gnupg"),
    app!("Security & privacy", "Pass", "", "pass"),
    app!("Security & privacy", "gopass", "", "gopass"),
    app!(
        "Security & privacy",
        "Authenticator",
        "com.belmoussaoui.Authenticator.desktop",
        "gnome-authenticator"
    ),
    app!(
        "Security & privacy",
        "Secrets",
        "org.gnome.World.Secrets.desktop",
        "gnome-secrets"
    ),
    app!(
        "Security & privacy",
        "Password Safe",
        "pwsafe.desktop",
        "passwordsafe"
    ),
    app!("Security & privacy", "KGpg", "org.kde.kgpg.desktop", "kgpg"),
    app!(
        "Security & privacy",
        "zuluCrypt",
        "zuluCrypt.desktop",
        "zulucrypt-gui"
    ),
    app!("Security & privacy", "UFW", "", "ufw"),
    app!("Security & privacy", "cryptsetup", "", "cryptsetup"),
    app!("Reverse engineering", "Ghidra", "", "ghidra"),
    app!(
        "Reverse engineering",
        "Cutter",
        "re.rizin.cutter.desktop",
        "cutter"
    ),
    app!("Reverse engineering", "Rizin", "", "rizin"),
    app!("Reverse engineering", "radare2", "", "radare2"),
    app!("Reverse engineering", "ImHex", "imhex.desktop", "imhex"),
    app!("Reverse engineering", "JADX", "jadx-gui.desktop", "jadx"),
    app!("Reverse engineering", "Apktool", "", "Apktool"),
    app!("Reverse engineering", "boomerang", "", "boomerang"),
    app!(
        "Reverse engineering",
        "EDB Debugger",
        "edb.desktop",
        "edb-debugger"
    ),
    app!(
        "Reverse engineering",
        "Okteta",
        "org.kde.okteta.desktop",
        "okteta"
    ),
    app!("Reverse engineering", "dhex", "", "dhex"),
    app!("Reverse engineering", "hexer", "", "hexer"),
    app!("Reverse engineering", "BIEW", "", "biew"),
    app!("Reverse engineering", "strace", "", "strace"),
    app!("Reverse engineering", "ltrace", "", "ltrace"),
    app!("Reverse engineering", "Binwalk", "", "binwalk"),
    app!(
        "Reverse engineering",
        "UEFITool",
        "uefitool.desktop",
        "UEFITool"
    ),
    app!("Reverse engineering", "The Sleuth Kit", "", "sleuthkit"),
    app!("Reverse engineering", "Foremost", "", "foremost"),
    app!(
        "Virtualization & containers",
        "Virtual Machine Manager",
        "virt-manager.desktop",
        "virt-manager"
    ),
    app!(
        "Virtualization & containers",
        "Virtual Machine Viewer",
        "remote-viewer.desktop",
        "virt-viewer"
    ),
    app!("Virtualization & containers", "QEMU", "", "qemu"),
    app!("Virtualization & containers", "Docker", "", "docker"),
    app!(
        "Virtualization & containers",
        "Docker Compose",
        "",
        "docker-compose"
    ),
    app!("Virtualization & containers", "Podman", "", "podman"),
    app!("Virtualization & containers", "Buildah", "", "buildah"),
    app!("Virtualization & containers", "Skopeo", "", "skopeo"),
    app!(
        "Virtualization & containers",
        "Waydroid",
        "Waydroid.desktop",
        "waydroid"
    ),
    app!("Terminals", "Alacritty", "Alacritty.desktop", "alacritty"),
    app!("Terminals", "Foot", "foot.desktop", "foot"),
    app!(
        "Terminals",
        "Ghostty",
        "com.mitchellh.ghostty.desktop",
        "ghostty"
    ),
    app!("Terminals", "Kitty", "kitty.desktop", "kitty"),
    app!(
        "Terminals",
        "WezTerm",
        "org.wezfurlong.wezterm.desktop",
        "wezterm"
    ),
    app!(
        "Terminals",
        "GNOME Terminal",
        "org.gnome.Terminal.desktop",
        "gnome-terminal"
    ),
    app!("Terminals", "Konsole", "org.kde.konsole.desktop", "konsole"),
    app!(
        "Terminals",
        "Xfce Terminal",
        "xfce4-terminal.desktop",
        "xfce4-terminal"
    ),
    app!("Terminals", "QTerminal", "qterminal.desktop", "qterminal"),
    app!(
        "Terminals",
        "LXTerminal",
        "lxterminal.desktop",
        "lxterminal"
    ),
    app!(
        "Terminals",
        "MATE Terminal",
        "mate-terminal.desktop",
        "mate-terminal"
    ),
    app!(
        "Terminals",
        "Terminator",
        "terminator.desktop",
        "terminator"
    ),
    app!(
        "Terminals",
        "Terminology",
        "terminology.desktop",
        "terminology"
    ),
    app!(
        "Terminals",
        "Cool Retro Term",
        "cool-retro-term.desktop",
        "cool-retro-term"
    ),
    app!("Terminals", "Guake", "guake.desktop", "guake"),
    app!("Terminals", "Yakuake", "org.kde.yakuake.desktop", "yakuake"),
    app!("Terminals", "XTerm", "xterm.desktop", "xterm"),
    app!(
        "Terminals",
        "Rxvt Unicode",
        "rxvt-unicode.desktop",
        "rxvt-unicode"
    ),
    app!("Terminals", "tmux", "", "tmux"),
    app!("Terminals", "GNU Screen", "", "screen"),
    app!("Files & storage", "Caja", "caja.desktop", "caja"),
    app!("Files & storage", "Nemo", "nemo.desktop", "nemo"),
    app!(
        "Files & storage",
        "Krusader",
        "org.kde.krusader.desktop",
        "krusader"
    ),
    app!("Files & storage", "SpaceFM", "spacefm.desktop", "spacefm"),
    app!(
        "Files & storage",
        "PCManFM-Qt",
        "pcmanfm-qt.desktop",
        "pcmanfm-qt"
    ),
    app!("Files & storage", "Midnight Commander", "", "mc"),
    app!("Files & storage", "Ranger", "ranger.desktop", "ranger"),
    app!("Files & storage", "Vifm", "vifm.desktop", "vifm"),
    app!("Files & storage", "Yazi", "yazi.desktop", "yazi"),
    app!("Files & storage", "lf", "lf.desktop", "lf"),
    app!("Files & storage", "ncdu", "", "ncdu"),
    app!("Files & storage", "gdu", "", "gdu"),
    app!("Files & storage", "duf", "", "duf"),
    app!(
        "Files & storage",
        "FSearch",
        "io.github.cboxdoerfer.FSearch.desktop",
        "fsearch"
    ),
    app!(
        "Files & storage",
        "ANGRYsearch",
        "angrysearch.desktop",
        "angrysearch"
    ),
    app!(
        "Files & storage",
        "Xarchiver",
        "xarchiver.desktop",
        "xarchiver"
    ),
    app!(
        "Files & storage",
        "LXQt Archiver",
        "lxqt-archiver.desktop",
        "lxqt-archiver"
    ),
    app!("Files & storage", "7-Zip", "", "7zip"),
    app!("Files & storage", "Rclone", "", "rclone"),
    app!("Files & storage", "rsync", "", "rsync"),
    app!("Files & storage", "Unison", "", "unison"),
    app!("Accessibility", "Orca", "", "orca"),
    app!("Accessibility", "Onboard", "onboard.desktop", "onboard"),
    app!("Accessibility", "wvkbd", "", "wvkbd"),
    app!(
        "Accessibility",
        "KMagnifier",
        "org.kde.kmag.desktop",
        "kmag"
    ),
    app!("Accessibility", "eSpeak NG", "", "espeak-ng"),
    app!("Development", "CodeLite", "codelite.desktop", "codelite"),
    app!("Development", "Lapce", "dev.lapce.lapce.desktop", "lapce"),
    app!("Development", "Anjuta", "anjuta.desktop", "anjuta"),
    app!(
        "Development",
        "IntelliJ IDEA Community",
        "idea.desktop",
        "intellij-idea-community-edition"
    ),
    app!(
        "Development",
        "PyCharm Community",
        "pycharm.desktop",
        "pycharm-community"
    ),
    app!("Development", "Lazarus", "lazarus.desktop", "lazarus"),
    app!("Development", "SASM", "sasm.desktop", "sasm"),
    app!("Development", "Lazygit", "", "lazygit"),
    app!("Development", "Tig", "", "tig"),
    app!("Graphics", "Geeqie", "org.geeqie.Geeqie.desktop", "geeqie"),
    app!("Graphics", "feh", "feh.desktop", "feh"),
    app!("Graphics", "imv", "imv.desktop", "imv"),
    app!("Graphics", "nsxiv", "nsxiv.desktop", "nsxiv"),
    app!("Graphics", "mtPaint", "mtpaint.desktop", "mtpaint"),
    app!(
        "Graphics",
        "qView",
        "com.interversehq.qView.desktop",
        "qView"
    ),
    app!("Graphics", "qimgv", "qimgv.desktop", "qimgv"),
    app!("Sound", "Amberol", "io.bassi.Amberol.desktop", "amberol"),
    app!(
        "Sound",
        "Cantata",
        "dog.unix.cantata.Cantata.desktop",
        "cantata"
    ),
    app!("Sound", "cmus", "", "cmus"),
    app!("Sound", "EasyTAG", "easytag.desktop", "easytag"),
    app!("Sound", "Ear Tag", "app.drey.EarTag.desktop", "eartag"),
    app!("Sound", "Elisa", "org.kde.elisa.desktop", "elisa"),
    app!("Sound", "fooyin", "org.fooyin.fooyin.desktop", "fooyin"),
    app!("Sound", "Puddletag", "puddletag.desktop", "puddletag"),
    app!("Sound", "Qmmp", "qmmp.desktop", "qmmp"),
    app!("Video", "Aegisub", "org.aegisub.Aegisub.desktop", "Aegisub"),
    app!("Video", "Kooha", "io.github.seadve.Kooha.desktop", "Kooha"),
    app!(
        "Video",
        "Clapper",
        "com.github.rafostar.Clapper.desktop",
        "clapper"
    ),
    app!(
        "Video",
        "Flowblade",
        "io.github.jliljebl.Flowblade.desktop",
        "flowblade"
    ),
    app!("Video", "Haruna", "org.kde.haruna.desktop", "haruna"),
    app!(
        "Video",
        "SimpleScreenRecorder",
        "simplescreenrecorder.desktop",
        "ssr"
    ),
    app!(
        "Video",
        "Subtitle Editor",
        "org.kitone.subtitleeditor.desktop",
        "subtitleeditor"
    ),
    app!("Video", "wf-recorder", "", "wf-recorder"),
    app!(
        "Office & documents",
        "Papers",
        "org.gnome.Papers.desktop",
        "papers"
    ),
    app!("Office & documents", "Atril", "atril.desktop", "atril"),
    app!(
        "Office & documents",
        "qpdfview",
        "qpdfview.desktop",
        "qpdfview"
    ),
    app!("Office & documents", "Sigil", "sigil.desktop", "sigil"),
    app!("Office & documents", "Sioyek", "sioyek.desktop", "sioyek"),
    app!("Office & documents", "Typst", "", "typst"),
    app!(
        "Office & documents",
        "VisiData",
        "visidata.desktop",
        "visidata"
    ),
    app!(
        "Notes & planning",
        "Planify",
        "io.github.alainm23.planify.desktop",
        "planify"
    ),
    app!(
        "Notes & planning",
        "Merkuro Calendar",
        "org.kde.merkuro.calendar.desktop",
        "merkuro"
    ),
    app!(
        "Notes & planning",
        "Orage",
        "org.xfce.orage.desktop",
        "orage"
    ),
    app!(
        "Notes & planning",
        "Zanshin",
        "org.kde.zanshin.desktop",
        "zanshin"
    ),
    app!("Notes & planning", "Taskwarrior", "", "task"),
    app!("Notes & planning", "Todoman", "", "todoman"),
    app!("Finance", "Eqonomize!", "eqonomize.desktop", "eqonomize"),
    app!("Finance", "Beancount", "", "beancount"),
    app!("Finance", "hledger", "", "hledger"),
    app!("Finance", "Ledger", "", "ledger"),
    app!(
        "News & podcasts",
        "GNOME Feeds",
        "org.gabmus.gfeeds.desktop",
        "gfeeds"
    ),
    app!("News & podcasts", "Kasts", "org.kde.kasts.desktop", "kasts"),
    app!("News & podcasts", "Newsboat", "", "newsboat"),
    app!(
        "News & podcasts",
        "NewsFlash",
        "io.gitlab.news_flash.NewsFlash.desktop",
        "newsflash"
    ),
    app!(
        "News & podcasts",
        "Newsraft",
        "newsraft.desktop",
        "newsraft"
    ),
    app!(
        "CAD & engineering",
        "Dune 3D",
        "org.dune3d.dune3d.desktop",
        "dune3d"
    ),
    app!("CAD & engineering", "LeoCAD", "leocad.desktop", "leocad"),
    app!("CAD & engineering", "ngspice", "", "ngspice"),
    app!("Education", "GNU Typist", "", "gtypist"),
    app!("Education", "Klavaro", "klavaro.desktop", "klavaro"),
    app!("Network", "bmon", "", "bmon"),
    app!("Network", "FreeRDP", "", "freerdp"),
    app!("Network", "rdesktop", "", "rdesktop"),
    app!("Network", "Vinagre", "vinagre.desktop", "vinagre"),
    app!("Network", "netscanner", "", "netscanner"),
    app!(
        "Emulation",
        "RetroArch",
        "com.libretro.RetroArch.desktop",
        "retroarch"
    ),
    app!(
        "Emulation",
        "Dolphin Emulator",
        "dolphin-emu.desktop",
        "dolphin-emu"
    ),
    app!(
        "Emulation",
        "DOSBox Staging",
        "org.dosbox-staging.dosbox-staging.desktop",
        "dosbox-staging"
    ),
    app!("Emulation", "DOSBox", "dosbox.desktop", "dosbox"),
    app!("Emulation", "PPSSPP", "PPSSPPSDL.desktop", "ppsspp"),
    app!("Emulation", "mGBA", "io.mgba.mGBA.desktop", "mgba-qt"),
    app!(
        "Emulation",
        "VisualBoyAdvance-M",
        "visualboyadvance-m.desktop",
        "vba-m"
    ),
    app!(
        "Emulation",
        "melonDS",
        "net.kuribo64.melonDS.desktop",
        "melonDS"
    ),
    app!("Emulation", "DeSmuME", "", "desmume"),
    app!("Emulation", "PCSX-Reloaded", "pcsxr.desktop", "pcsxr"),
    app!("Emulation", "Snes9x", "snes9x-gtk.desktop", "snes9x-gtk"),
    app!("Emulation", "higan", "higan.desktop", "higan"),
    app!("Emulation", "FCEUX", "fceux.desktop", "fceux"),
    app!(
        "Emulation",
        "Mupen64Plus",
        "mupen64plus.desktop",
        "mupen64plus"
    ),
    app!("Emulation", "Mednaffe", "mednaffe.desktop", "mednaffe"),
    app!("Emulation", "MAME", "", "mame"),
    app!(
        "Emulation",
        "ScummVM",
        "org.scummvm.scummvm.desktop",
        "scummvm"
    ),
    app!("Emulation", "FS-UAE", "fs-uae.desktop", "fs-uae"),
    app!("Emulation", "Hatari", "hatari.desktop", "hatari"),
    app!("Emulation", "VICE", "vice-org-x64sc.desktop", "vice"),
    app!("Emulation", "Stella", "stella.desktop", "stella"),
    app!("Emulation", "DGen", "", "dgen-sdl"),
    app!("Emulation", "Bochs", "", "bochs"),
    app!("Emulation", "EmulationStation", "", "EmulationStation"),
    app!(
        "Desktop & appearance",
        "GNOME Tweaks",
        "org.gnome.tweaks.desktop",
        "gnome-tweaks"
    ),
    app!(
        "Desktop & appearance",
        "Extension Manager",
        "com.mattjakeman.ExtensionManager.desktop",
        "extension-manager"
    ),
    app!(
        "Desktop & appearance",
        "GNOME Shell Extensions",
        "",
        "gnome-shell-extensions"
    ),
    app!(
        "Desktop & appearance",
        "LXAppearance",
        "lxappearance.desktop",
        "lxappearance"
    ),
    app!(
        "Desktop & appearance",
        "nwg-look",
        "nwg-look.desktop",
        "nwg-look"
    ),
    app!(
        "Desktop & appearance",
        "Qt5 Configuration Tool",
        "qt5ct.desktop",
        "qt5ct"
    ),
    app!(
        "Desktop & appearance",
        "Qt6 Configuration Tool",
        "qt6ct.desktop",
        "qt6ct"
    ),
    app!(
        "Desktop & appearance",
        "Kvantum Manager",
        "kvantummanager.desktop",
        "kvantum"
    ),
    app!(
        "Desktop & appearance",
        "Font Manager",
        "com.github.FontManager.FontManager.desktop",
        "fontmanager"
    ),
    app!(
        "Desktop & appearance",
        "GNOME Fonts",
        "org.gnome.font-viewer.desktop",
        "gnome-font-viewer"
    ),
    app!(
        "Desktop & appearance",
        "Variety",
        "variety.desktop",
        "variety"
    ),
    app!(
        "Desktop & appearance",
        "Nitrogen",
        "nitrogen.desktop",
        "nitrogen"
    ),
    app!("Desktop & appearance", "Azote", "azote.desktop", "azote"),
    app!("Desktop & appearance", "ARandR", "arandr.desktop", "arandr"),
    app!(
        "Desktop & appearance",
        "wdisplays",
        "network.cycles.wdisplays.desktop",
        "wdisplays"
    ),
    app!(
        "Desktop & appearance",
        "Xfce Settings",
        "xfce-settings-manager.desktop",
        "xfce4-settings"
    ),
    app!(
        "Desktop & appearance",
        "LXQt Configuration Center",
        "lxqt-config.desktop",
        "lxqt-config"
    ),
    app!(
        "Printing & scanning",
        "Print Settings",
        "system-config-printer.desktop",
        "system-config-printer"
    ),
    app!(
        "Printing & scanning",
        "KDE Print Manager",
        "org.kde.ConfigurePrinter.desktop",
        "print-manager"
    ),
    app!("Printing & scanning", "CUPS", "cups.desktop", "cups"),
    app!("Printing & scanning", "HPLIP", "", "hplip"),
    app!("Printing & scanning", "Gutenprint", "", "gutenprint"),
    app!(
        "Printing & scanning",
        "Skanlite",
        "org.kde.skanlite.desktop",
        "skanlite"
    ),
    app!(
        "Printing & scanning",
        "Skanpage",
        "org.kde.skanpage.desktop",
        "skanpage"
    ),
    app!("Printing & scanning", "XSane", "xsane.desktop", "xsane"),
    app!(
        "Printing & scanning",
        "gscan2pdf",
        "net.sourceforge.gscan2pdf.desktop",
        "gscan2pdf"
    ),
    app!("Web", "Konqueror", "org.kde.konqueror.desktop", "konqueror"),
    app!("Web", "Badwolf", "badwolf.desktop", "badwolf"),
    app!("Web", "Luakit", "luakit.desktop", "luakit"),
    app!("Web", "Vimb", "vimb.desktop", "vimb"),
    app!("Web", "surf", "", "surf"),
    app!("Web", "Nyxt", "nyxt.desktop", "nyxt"),
    app!(
        "Web",
        "Tor Browser Launcher",
        "org.torproject.torbrowser-launcher.desktop",
        "torbrowser-launcher"
    ),
    app!("Web", "Kristall", "Kristall.desktop", "kristall"),
    app!("Web", "Lynx", "", "lynx"),
    app!("Web", "ELinks", "", "elinks"),
    app!(
        "Mail & communication",
        "Balsa",
        "org.desktop.Balsa.desktop",
        "balsa"
    ),
    app!("Mail & communication", "aerc", "aerc.desktop", "aerc"),
    app!("Mail & communication", "Alpine", "", "alpine"),
    app!(
        "Mail & communication",
        "Himalaya",
        "himalaya.desktop",
        "himalaya"
    ),
    app!(
        "Mail & communication",
        "Fractal",
        "org.gnome.Fractal.desktop",
        "fractal"
    ),
    app!(
        "Mail & communication",
        "Tuba",
        "dev.geopjr.Tuba.desktop",
        "Tuba"
    ),
    app!(
        "Mail & communication",
        "Tokodon",
        "org.kde.tokodon.desktop",
        "tokodon"
    ),
    app!(
        "Mail & communication",
        "Mumble",
        "info.mumble.Mumble.desktop",
        "mumble"
    ),
    app!(
        "Mail & communication",
        "qTox",
        "io.github.qtox.qTox.desktop",
        "qtox"
    ),
    app!("Mail & communication", "Toxic", "toxic.desktop", "toxic"),
    app!(
        "Mail & communication",
        "Chatterino",
        "com.chatterino.chatterino.desktop",
        "chatterino2"
    ),
    app!("Mail & communication", "Irssi", "", "irssi"),
    app!("Mail & communication", "senpai", "", "senpai"),
    app!("Mail & communication", "Profanity", "", "profanity"),
    app!(
        "Mail & communication",
        "GNOME Contacts",
        "org.gnome.Contacts.desktop",
        "gnome-contacts"
    ),
    app!("News & podcasts", "Castero", "", "castero"),
    app!("News & podcasts", "Canto", "", "canto-curses"),
    app!(
        "Office & documents",
        "Xreader",
        "xreader.desktop",
        "xreader"
    ),
    app!("Office & documents", "MuPDF", "mupdf.desktop", "mupdf"),
    app!(
        "Office & documents",
        "ePDFView",
        "epdfview.desktop",
        "epdfview"
    ),
    app!("Office & documents", "GNU GV", "", "gv"),
    app!("Office & documents", "Katarakt", "", "katarakt"),
    app!(
        "Office & documents",
        "FBReader",
        "FBReader.desktop",
        "fbreader"
    ),
    app!("Office & documents", "Calligra", "", "calligra"),
    app!("Office & documents", "LyX", "lyx.desktop", "lyx"),
    app!(
        "Office & documents",
        "Texmaker",
        "texmaker.desktop",
        "texmaker"
    ),
    app!("Office & documents", "Gummi", "gummi.desktop", "gummi"),
    app!(
        "Office & documents",
        "Apostrophe",
        "org.gnome.gitlab.somas.Apostrophe.desktop",
        "apostrophe"
    ),
    app!("Office & documents", "sc-im", "", "sc-im"),
    app!("Office & documents", "WordGrinder", "", "wordgrinder"),
    app!("Office & documents", "epy", "", "epy"),
    app!(
        "Notes & planning",
        "Endeavour",
        "org.gnome.Todo.desktop",
        "Endeavour"
    ),
    app!("Notes & planning", "zk", "", "zk"),
    app!("Finance", "Electrum", "electrum.desktop", "electrum"),
    app!("Finance", "hledger-ui", "", "hledger-ui"),
    app!("Editors", "FeatherPad", "featherpad.desktop", "FeatherPad"),
    app!("Editors", "Pluma", "pluma.desktop", "pluma"),
    app!("Editors", "Notepadqq", "notepadqq.desktop", "notepadqq"),
    app!(
        "Editors",
        "Code",
        "io.elementary.code.desktop",
        "io.elementary.code"
    ),
    app!(
        "Editors",
        "Sublime Text",
        "sublime_text.desktop",
        "sublime-text4"
    ),
    app!("Editors", "Neovide", "neovide.desktop", "neovide"),
    app!("Editors", "Kakoune", "", "kakoune"),
    app!("Editors", "JOE", "joe.desktop", "joe"),
    app!("Editors", "mg", "", "mg"),
    app!("Editors", "xed", "", "xed"),
    app!(
        "Graphics",
        "KolourPaint",
        "org.kde.kolourpaint.desktop",
        "kolourpaint"
    ),
    app!("Graphics", "GPicView", "gpicview.desktop", "gpicview"),
    app!("Graphics", "LXImage-Qt", "lximage-qt.desktop", "lximage-qt"),
    app!("Graphics", "Eye of MATE", "eom.desktop", "eom"),
    app!("Graphics", "swayimg", "swayimg.desktop", "swayimg"),
    app!("Graphics", "pqiv", "pqiv.desktop", "pqiv"),
    app!("Graphics", "qiv", "qiv.desktop", "qiv"),
    app!("Graphics", "vimiv", "vimiv.desktop", "vimiv"),
    app!("Graphics", "Phototonic", "phototonic.desktop", "phototonic"),
    app!(
        "Graphics",
        "Converseen",
        "net.fasterland.converseen.desktop",
        "Converseen"
    ),
    app!(
        "Graphics",
        "Gcolor3",
        "nl.hjdskes.gcolor3.desktop",
        "gcolor3"
    ),
    app!("Graphics", "Gpick", "org.gpick.gpick.desktop", "gpick"),
    app!("Graphics", "ksnip", "org.ksnip.ksnip.desktop", "ksnip"),
    app!("Graphics", "Satty", "satty.desktop", "satty"),
    app!("Graphics", "Swappy", "swappy.desktop", "swappy"),
    app!("Graphics", "Potrace", "", "potrace"),
    app!(
        "Sound",
        "GNOME Music",
        "org.gnome.Music.desktop",
        "gnome-music"
    ),
    app!(
        "Sound",
        "Sound Recorder",
        "org.gnome.SoundRecorder.desktop",
        "gnome-sound-recorder"
    ),
    app!("Sound", "Pragha", "pragha.desktop", "pragha"),
    app!("Sound", "Sonata", "sonata.desktop", "sonata"),
    app!("Sound", "Ymuse", "com.yktoo.ymuse.desktop", "ymuse"),
    app!("Sound", "spotify-qt", "spotify-qt.desktop", "spotify-qt"),
    app!("Sound", "Music Player Daemon", "", "mpd"),
    app!("Sound", "ncmpcpp", "", "ncmpcpp"),
    app!("Sound", "kew", "kew.desktop", "kew"),
    app!("Sound", "musikcube", "musikcube.desktop", "musikcube"),
    app!("Sound", "Tag Editor", "tageditor.desktop", "tageditor"),
    app!("Sound", "Carla", "carla.desktop", "Carla"),
    app!("Sound", "Guitarix", "guitarix.desktop", "guitarix2"),
    app!("Sound", "amsynth", "amsynth.desktop", "amsynth"),
    app!(
        "Sound",
        "ZynAddSubFX",
        "zynaddsubfx-jack.desktop",
        "zynaddsubfx"
    ),
    app!("Sound", "synthv1", "org.rncbc.synthv1.desktop", "synthv1"),
    app!("Sound", "Geonkick", "geonkick.desktop", "geonkick"),
    app!("Video", "Totem", "org.gnome.Totem.desktop", "totem"),
    app!(
        "Video",
        "Showtime",
        "org.gnome.Showtime.desktop",
        "showtime"
    ),
    app!("Video", "Kaffeine", "org.kde.kaffeine.desktop", "kaffeine"),
    app!(
        "Video",
        "MPC-QT",
        "io.github.mpc_qt.mpc-qt.desktop",
        "mpc-qt"
    ),
    app!("Video", "QMPlay2", "QMPlay2.desktop", "QMPlay2"),
    app!(
        "Video",
        "PlasmaTube",
        "org.kde.plasmatube.desktop",
        "plasmatube"
    ),
    app!(
        "Video",
        "Avidemux",
        "org.avidemux.Avidemux.desktop",
        "avidemux"
    ),
    app!(
        "Video",
        "MKVToolNix",
        "org.bunkus.mkvtoolnix-gui.desktop",
        "mkvtoolnix-gui"
    ),
    app!(
        "Video",
        "GNOME Subtitles",
        "org.gnome.GnomeSubtitles.desktop",
        "gnome-subtitles"
    ),
    app!(
        "Video",
        "vokoscreenNG",
        "vokoscreenNG.desktop",
        "vokoscreenNG"
    ),
    app!("Video", "Peek", "com.uploadedlobster.peek.desktop", "peek"),
    app!("Video", "GPU Screen Recorder", "", "gpu-screen-recorder"),
    app!("Games", "Xonotic", "xonotic-glx.desktop", "xonotic"),
    app!("Games", "OpenRA", "openra-ra.desktop", "openra"),
    app!(
        "Games",
        "Endless Sky",
        "io.github.endless_sky.endless_sky.desktop",
        "endless-sky"
    ),
    app!("Games", "OpenMW", "org.openmw.launcher.desktop", "openmw"),
    app!("Games", "Teeworlds", "teeworlds.desktop", "teeworlds"),
    app!("Games", "Pingus", "pingus.desktop", "pingus"),
    app!(
        "Games",
        "Chromium B.S.U.",
        "chromium-bsu.desktop",
        "chromium-bsu"
    ),
    app!("Games", "X-Moto", "xmoto.desktop", "xmoto"),
    app!("Games", "Flare", "", "flare-game"),
    app!(
        "Games",
        "The Powder Toy",
        "powder.desktop",
        "The-Powder-Toy"
    ),
    app!("Games", "Cataclysm: Dark Days Ahead", "", "Cataclysm-DDA"),
    app!("Games", "Dungeon Crawl Stone Soup", "", "crawl"),
    app!("Games", "NetHack", "", "nethack"),
    app!(
        "Games",
        "Prism Launcher",
        "org.prismlauncher.PrismLauncher.desktop",
        "PrismLauncher"
    ),
    app!("Games", "KPatience", "org.kde.kpat.desktop", "kpat"),
    app!("Games", "KMines", "org.kde.kmines.desktop", "kmines"),
    app!(
        "Games",
        "GNOME 2048",
        "org.gnome.TwentyFortyEight.desktop",
        "gnome-2048"
    ),
    app!("Games", "Atomix", "atomix.desktop", "atomix"),
    app!("Games", "Hitori", "org.gnome.Hitori.desktop", "hitori"),
    app!("Games", "Reversi", "org.gnome.Reversi.desktop", "iagno"),
    app!(
        "Games",
        "Klotski",
        "org.gnome.Klotski.desktop",
        "gnome-klotski"
    ),
    app!(
        "Games",
        "Lights Off",
        "org.gnome.LightsOff.desktop",
        "lightsoff"
    ),
    app!(
        "Games",
        "Nibbles",
        "org.gnome.Nibbles.desktop",
        "gnome-nibbles"
    ),
    app!(
        "Games",
        "Robots",
        "org.gnome.Robots.desktop",
        "gnome-robots"
    ),
    app!(
        "Games",
        "Swell Foop",
        "org.gnome.SwellFoop.desktop",
        "swell-foop"
    ),
    app!("Games", "Tali", "org.gnome.Tali.desktop", "tali"),
    app!("Games", "GameMode", "", "gamemode"),
    app!("Games", "MangoHud", "", "MangoHud"),
    app!("Science & mathematics", "Xcas", "xcas.desktop", "giac"),
    app!("Science & mathematics", "SageMath", "", "sagemath"),
    app!("Science & mathematics", "Kig", "org.kde.kig.desktop", "kig"),
    app!(
        "Science & mathematics",
        "KmPlot",
        "org.kde.kmplot.desktop",
        "kmplot"
    ),
    app!("Science & mathematics", "PyMOL", "pymol.desktop", "pymol"),
    app!(
        "Science & mathematics",
        "Gpredict",
        "gpredict.desktop",
        "gpredict"
    ),
    app!(
        "Science & mathematics",
        "GNU Radio Companion",
        "gnuradio-grc.desktop",
        "gnuradio"
    ),
    app!("Science & mathematics", "GRASS GIS", "", "grass"),
    app!("Science & mathematics", "Gmsh", "", "gmsh"),
    app!(
        "Science & mathematics",
        "Galculator",
        "galculator.desktop",
        "galculator"
    ),
    app!(
        "Science & mathematics",
        "KCalc",
        "org.kde.kcalc.desktop",
        "kcalc"
    ),
    app!(
        "CAD & engineering",
        "LibreCAD",
        "librecad.desktop",
        "LibreCAD"
    ),
    app!(
        "CAD & engineering",
        "Horizon EDA",
        "org.horizon_eda.HorizonEDA.desktop",
        "horizon"
    ),
    app!(
        "CAD & engineering",
        "Fritzing",
        "fritzing.desktop",
        "Fritzing"
    ),
    app!(
        "CAD & engineering",
        "Freerouting",
        "freerouting.desktop",
        "freerouting"
    ),
    app!(
        "CAD & engineering",
        "PulseView",
        "org.sigrok.PulseView.desktop",
        "pulseview"
    ),
    app!("CAD & engineering", "sigrok-cli", "", "sigrok-cli"),
    app!("CAD & engineering", "GTKWave", "gtkwave.desktop", "gtkwave"),
    app!(
        "CAD & engineering",
        "Arduino IDE",
        "arduino.desktop",
        "arduino"
    ),
    app!("CAD & engineering", "OpenOCD", "", "openocd"),
    app!("CAD & engineering", "probe-rs", "", "probe-rs"),
    app!("Development", "Eclipse", "eclipse.desktop", "eclipse"),
    app!("Development", "LiteIDE", "liteide.desktop", "liteide"),
    app!("Development", "juCi++", "juci.desktop", "juCi++"),
    app!("Development", "IDLE", "idle3.desktop", "idle-python3"),
    app!("Development", "DBeaver", "dbeaver.desktop", "dbeaver"),
    app!("Development", "Zeal", "org.zealdocs.zeal.desktop", "zeal"),
    app!(
        "Development",
        "Sublime Merge",
        "sublime_merge.desktop",
        "sublime-merge"
    ),
    app!("Development", "GitUI", "", "gitui"),
    app!("Development", "gitu", "", "gitu"),
    app!("Development", "GitHub CLI", "", "github-cli"),
    app!("Development", "GitLab CLI", "", "glab"),
    app!("Development", "Valgrind", "", "valgrind"),
    app!("Development", "Heaptrack", "", "heaptrack"),
    app!(
        "Development",
        "Hotspot",
        "com.kdab.hotspot.desktop",
        "hotspot"
    ),
    app!("Development", "hyperfine", "", "hyperfine"),
    app!("Development", "D-Spy", "org.gnome.dfeet.desktop", "d-feet"),
    app!("Reverse engineering", "cgdb", "", "cgdb"),
    app!(
        "Reverse engineering",
        "GDB (all architectures)",
        "",
        "gdb-multiarch"
    ),
    app!("Reverse engineering", "GEF", "", "gef"),
    app!("Reverse engineering", "rr", "", "rr"),
    app!(
        "Reverse engineering",
        "RenderDoc",
        "renderdoc.desktop",
        "renderdoc"
    ),
    app!("Reverse engineering", "bpftrace", "", "bpftrace"),
    app!("Reverse engineering", "hyx", "", "hyx"),
    app!("Reverse engineering", "tweak", "", "tweak"),
    app!("Reverse engineering", "smali", "", "smali"),
    app!("Reverse engineering", "xtruss", "", "xtruss"),
    app!("Reverse engineering", "wev", "", "wev"),
    app!(
        "Virtualization & containers",
        "GNOME Boxes",
        "org.gnome.Boxes.desktop",
        "gnome-boxes"
    ),
    app!(
        "Virtualization & containers",
        "VirtualBox",
        "virtualbox.desktop",
        "virtualbox-ose"
    ),
    app!("Virtualization & containers", "Incus", "", "incus"),
    app!("Virtualization & containers", "LXD", "", "lxd"),
    app!("Virtualization & containers", "LXC", "", "lxc"),
    app!("Virtualization & containers", "Vagrant", "", "vagrant"),
    app!(
        "Virtualization & containers",
        "Podman Compose",
        "",
        "podman-compose"
    ),
    app!(
        "Virtualization & containers",
        "Podman TUI",
        "",
        "podman-tui"
    ),
    app!(
        "Virtualization & containers",
        "lazydocker",
        "",
        "lazydocker"
    ),
    app!("Virtualization & containers", "ctop", "", "ctop"),
    app!("Virtualization & containers", "dive", "", "dive"),
    app!("Virtualization & containers", "kubectl", "", "kubectl"),
    app!("Virtualization & containers", "k9s", "", "k9s"),
    app!("Virtualization & containers", "minikube", "", "minikube"),
    app!(
        "Network",
        "Nextcloud",
        "com.nextcloud.desktopclient.nextcloud.desktop",
        "nextcloud-client"
    ),
    app!(
        "Network",
        "NetworkManager Applet",
        "nm-connection-editor.desktop",
        "network-manager-applet"
    ),
    app!("Network", "WireGuard Tools", "", "wireguard-tools"),
    app!("Network", "OpenVPN", "", "openvpn"),
    app!("Network", "X2Go Client", "x2goclient.desktop", "x2goclient"),
    app!("Network", "LinSSID", "linssid.desktop", "linssid"),
    app!("Network", "croc", "", "croc"),
    app!("Network", "Magic Wormhole", "", "magic-wormhole"),
    app!("Network", "qrcp", "", "qrcp"),
    app!("Network", "mtr", "", "mtr"),
    app!("Network", "Trippy", "", "trippy"),
    app!("Network", "nethogs", "", "nethogs"),
    app!("Network", "iftop", "", "iftop"),
    app!("Network", "bandwhich", "", "bandwhich"),
    app!("Network", "bwm-ng", "", "bwm-ng"),
    app!("Network", "Tor", "", "tor"),
    app!("Network", "torsocks", "", "torsocks"),
    app!(
        "Security & privacy",
        "BleachBit",
        "org.bleachbit.BleachBit.desktop",
        "bleachbit"
    ),
    app!("Security & privacy", "Gufw", "gufw.desktop", "gufw"),
    app!(
        "Security & privacy",
        "Plasma Firewall",
        "kcm_firewall.desktop",
        "plasma-firewall"
    ),
    app!("Security & privacy", "Firejail", "", "firejail"),
    app!("Security & privacy", "ClamAV", "", "clamav"),
    app!("Security & privacy", "Lynis", "", "lynis"),
    app!("Security & privacy", "Rootkit Hunter", "", "rkhunter"),
    app!(
        "Security & privacy",
        "Nitrokey App",
        "nitrokey-app.desktop",
        "nitrokey-app"
    ),
    app!(
        "Security & privacy",
        "YubiKey Manager",
        "",
        "yubikey-manager"
    ),
    app!("Security & privacy", "rage", "", "rage"),
    app!("Security & privacy", "Sequoia", "", "sequoia-sq"),
    app!("Security & privacy", "gocryptfs", "", "gocryptfs"),
    app!("Security & privacy", "CryFS", "", "cryfs"),
    app!("Security & privacy", "Tomb", "", "tomb"),
    app!("Security & privacy", "cotp", "", "cotp"),
    app!("Security & privacy", "pwgen", "", "pwgen"),
    app!("Security & privacy", "xkcdpass", "", "xkcdpass"),
    app!(
        "Security & privacy",
        "John the Ripper",
        "john.desktop",
        "john"
    ),
    app!("Security & privacy", "hashcat", "", "hashcat"),
    app!(
        "Files & storage",
        "Czkawka",
        "com.github.qarmin.czkawka.desktop",
        "czkawka"
    ),
    app!("Files & storage", "Xfe", "xfe.desktop", "xfe"),
    app!("Files & storage", "qtfm", "qtfm.desktop", "qtfm"),
    app!("Files & storage", "nnn", "nnn.desktop", "nnn"),
    app!("Files & storage", "clifm", "clifm.desktop", "clifm"),
    app!("Files & storage", "felix", "", "felix"),
    app!("Files & storage", "broot", "", "broot"),
    app!("Files & storage", "dua", "", "dua-cli"),
    app!("Files & storage", "btdu", "", "btdu"),
    app!("Files & storage", "fclones", "", "fclones"),
    app!("Files & storage", "jdupes", "", "jdupes"),
    app!("Files & storage", "GNU Parted", "", "parted"),
    app!("Files & storage", "dragon", "", "dragon"),
    app!("Terminals", "Ptyxis", "org.gnome.Ptyxis.desktop", "ptyxis"),
    app!("Terminals", "Rio", "rio.desktop", "rio"),
    app!(
        "Terminals",
        "Contour",
        "org.contourterminal.Contour.desktop",
        "contour"
    ),
    app!("Terminals", "Sakura", "sakura.desktop", "sakura"),
    app!("Terminals", "ROXTerm", "roxterm.desktop", "roxterm"),
    app!("Terminals", "Tilda", "tilda.desktop", "tilda"),
    app!("Terminals", "mlterm", "", "mlterm"),
    app!("Terminals", "st", "", "st"),
    app!("Terminals", "Zellij", "zellij.desktop", "zellij"),
    app!("Terminals", "Byobu", "", "byobu"),
    app!("Terminals", "CuteCom", "", "cutecom"),
    app!("Terminals", "Moserial", "moserial.desktop", "moserial"),
    app!("Accessibility", "Fenrir", "", "fenrir"),
    app!(
        "Accessibility",
        "Speech Dispatcher",
        "",
        "speech-dispatcher"
    ),
    app!("Accessibility", "Mimic", "", "mimic"),
    app!(
        "Utilities",
        "GNOME Weather",
        "org.gnome.Weather.desktop",
        "gnome-weather"
    ),
    app!(
        "Utilities",
        "GNOME Usage",
        "org.gnome.Usage.desktop",
        "gnome-usage"
    ),
    app!(
        "Utilities",
        "KSystemLog",
        "org.kde.ksystemlog.desktop",
        "ksystemlog"
    ),
    app!("Utilities", "bottom", "", "bottom"),
    app!("Utilities", "Glances", "", "glances"),
    app!("Utilities", "NVTOP", "nvtop.desktop", "nvtop"),
    app!(
        "Utilities",
        "CPU-X",
        "io.github.thetumultuousunicornofdarkness.cpu-x.desktop",
        "CPU-X"
    ),
    app!(
        "Utilities",
        "CoreCtrl",
        "org.corectrl.CoreCtrl.desktop",
        "corectrl"
    ),
    app!(
        "Utilities",
        "LACT",
        "io.github.ilya_zlobintsev.LACT.desktop",
        "LACT"
    ),
    app!(
        "Utilities",
        "GSmartControl",
        "gsmartcontrol.desktop",
        "gsmartcontrol"
    ),
    app!("Utilities", "fwupd", "", "fwupd"),
    app!(
        "Utilities",
        "Piper",
        "org.freedesktop.Piper.desktop",
        "piper"
    ),
    app!("Utilities", "Solaar", "solaar.desktop", "Solaar"),
    app!(
        "Utilities",
        "OpenRGB",
        "org.openrgb.OpenRGB.desktop",
        "openrgb"
    ),
    app!(
        "Utilities",
        "Character Map",
        "gucharmap.desktop",
        "gucharmap"
    ),
    app!(
        "Utilities",
        "KCharSelect",
        "org.kde.kcharselect.desktop",
        "kcharselect"
    ),
    app!(
        "Utilities",
        "CopyQ",
        "com.github.hluk.copyq.desktop",
        "CopyQ"
    ),
    app!(
        "Utilities",
        "Gramps",
        "org.gramps_project.Gramps.desktop",
        "gramps"
    ),
];

pub fn contains_package(name: &str) -> bool {
    static NAMES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            CATALOG
                .iter()
                .flat_map(|app| app.packages.iter().copied())
                .collect()
        })
        .contains(name)
}

/// Alias order expresses preference; installed-only or unavailable packages are excluded.
pub fn available_apps(packages: &[Package]) -> Vec<(&'static CatalogApp, &Package)> {
    let by_name: HashMap<_, _> = packages
        .iter()
        .filter(|pkg| pkg.version_available.is_some())
        .map(|pkg| (pkg.name.as_str(), pkg))
        .collect();
    CATALOG
        .iter()
        .filter_map(|app| {
            app.packages
                .iter()
                .find_map(|name| by_name.get(name).copied())
                .map(|pkg| (app, pkg))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_aliases_resolve_only_once_in_preferred_order() {
        let package = |name: &str| Package {
            name: name.into(),
            version_available: Some("1.0".into()),
            ..Package::default()
        };
        let packages = [
            package("libreoffice-writer"),
            package("libreoffice"),
            Package {
                version_available: None,
                ..package("gimp")
            },
            package("unlisted"),
        ];
        let available = available_apps(&packages);
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].1.name, "libreoffice");
        assert!(contains_package("libreoffice-writer"));
        assert!(!contains_package("unlisted"));
    }
    #[test]
    fn catalog_has_consistent_sections_and_unique_package_aliases() {
        let mut aliases = HashSet::new();
        let mut names = HashSet::new();
        let mut desktop_ids = HashSet::new();
        for app in CATALOG {
            assert!(CATEGORIES.contains(&app.category), "{}", app.name);
            assert!(names.insert(app.name), "duplicate app name: {}", app.name);
            if !app.desktop.is_empty() {
                assert!(app.desktop.ends_with(".desktop"), "{}", app.name);
                assert!(
                    desktop_ids.insert(app.desktop),
                    "duplicate desktop id: {}",
                    app.desktop
                );
            }
            assert!(!app.packages.is_empty());
            for name in app.packages {
                assert!(aliases.insert(name), "duplicate alias: {name}");
            }
        }
        for section in CATEGORIES {
            assert!(CATALOG.iter().any(|app| &app.category == section));
        }
        for name in FEATURED {
            assert!(
                CATALOG.iter().any(|app| app.name == *name),
                "unknown featured application: {name}"
            );
        }
    }
}
