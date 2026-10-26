//! A curated application index; names resolve only against the active backend.
//! Void candidates and desktop IDs can be checked with scripts/check-discover-void.py.
use crate::backend::package::Package;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub struct CatalogApp {
    pub category: &'static str,
    pub name: &'static str,
    pub packages: &'static [&'static str],
    pub desktop: &'static str,
}

pub const CATEGORIES: &[&str] = &[
    "Web",
    "Mail & communication",
    "Office & documents",
    "Editors",
    "Graphics",
    "Sound",
    "Video",
    "Games",
    "Education",
    "Science & mathematics",
    "Development",
    "Network",
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
        "Office & documents" => ("x-office-document-symbolic", "accent-orange"),
        "Editors" => ("text-editor-symbolic", "accent-slate"),
        "Graphics" => ("image-x-generic-symbolic", "accent-pink"),
        "Sound" => ("audio-x-generic-symbolic", "accent-green"),
        "Video" => ("video-x-generic-symbolic", "accent-red"),
        "Games" => ("applications-games-symbolic", "accent-purple"),
        "Education" => ("accessories-dictionary-symbolic", "accent-yellow"),
        "Science & mathematics" => ("applications-science-symbolic", "accent-teal"),
        "Development" => ("applications-engineering-symbolic", "accent-blue"),
        "Network" => ("network-workgroup-symbolic", "accent-green"),
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
        "Office & documents",
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
    app!("Graphics", "Darktable", "darktable.desktop", "darktable"),
    app!(
        "Graphics",
        "RawTherapee",
        "rawtherapee.desktop",
        "rawtherapee"
    ),
    app!("Graphics", "MyPaint", "mypaint.desktop", "mypaint"),
    app!("Graphics", "Pinta", "pinta.desktop", "pinta"),
    app!("Graphics", "gThumb", "gthumb.desktop", "gthumb"),
    app!("Graphics", "Gwenview", "gwenview.desktop", "gwenview"),
    app!("Graphics", "Shotwell", "shotwell.desktop", "shotwell"),
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
    app!("Video", "HandBrake", "HandBrake.desktop", "HandBrake"),
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
    app!("Games", "SuperTux", "supertux.desktop", "supertux"),
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
        "celestia.desktop",
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
        "wxmaxima.desktop",
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
    app!("Science & mathematics", "KiCad", "kicad.desktop", "kicad"),
    app!(
        "Science & mathematics",
        "FreeCAD",
        "FreeCAD.desktop",
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
        "Development",
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
    app!("Development", "Code - OSS", "code-oss.desktop", "code-oss"),
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
        "Utilities",
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
        "Office & documents",
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
        "Utilities",
        "GNOME Console",
        "org.gnome.Console.desktop",
        "gnome-console"
    ),
    app!(
        "Utilities",
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
    app!("Utilities", "GParted", "gparted.desktop", "gparted"),
    app!("Utilities", "Baobab", "baobab.desktop", "baobab"),
    app!(
        "Utilities",
        "File Roller",
        "file-roller.desktop",
        "file-roller"
    ),
    app!("Utilities", "Engrampa", "engrampa.desktop", "engrampa"),
    app!("Utilities", "KeepassXC", "keepassxc.desktop", "keepassxc"),
    app!("Utilities", "KeePass", "keepass.desktop", "keepass"),
    app!("Utilities", "Déjà Dup", "deja-dup.desktop", "deja-dup"),
    app!(
        "Utilities",
        "Back In Time",
        "backintime.desktop",
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
    app!("Utilities", "Thunar", "Thunar.desktop", "Thunar"),
    app!("Utilities", "Nautilus", "nautilus.desktop", "nautilus"),
    app!("Utilities", "Dolphin", "dolphin.desktop", "dolphin"),
    app!("Utilities", "PCManFM", "pcmanfm.desktop", "pcmanfm"),
    app!("Utilities", "Tilix", "tilix.desktop", "tilix"),
    app!("Utilities", "Tellico", "org.kde.tellico.desktop", "tellico"),
    app!("Utilities", "Ark", "org.kde.ark.desktop", "ark"),
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
        "Web",
        "Liferea",
        "net.sourceforge.liferea.desktop",
        "liferea"
    ),
    app!("Web", "Akregator", "org.kde.akregator.desktop", "akregator"),
    app!(
        "Web",
        "RSS Guard",
        "io.github.martinrotter.rssguardlite.desktop",
        "rssguard"
    ),
    app!(
        "Office & documents",
        "GnuCash",
        "gnucash.desktop",
        "gnucash"
    ),
    app!(
        "Office & documents",
        "HomeBank",
        "homebank.desktop",
        "homebank"
    ),
    app!(
        "Office & documents",
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
        "Office & documents",
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
        "Office & documents",
        "CherryTree",
        "cherrytree.desktop",
        "cherrytree"
    ),
    app!(
        "Office & documents",
        "Zim",
        "org.zim_wiki.Zim.desktop",
        "zim"
    ),
    app!(
        "Office & documents",
        "QOwnNotes",
        "PBE.QOwnNotes.desktop",
        "qownnotes"
    ),
    app!("Graphics", "digiKam", "org.kde.digikam.desktop", "digikam"),
    app!("Graphics", "Hugin", "hugin.desktop", "hugin"),
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
    app!("Sound", "gPodder", "gpodder.desktop", "gpodder"),
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
        "Science & mathematics",
        "OpenSCAD",
        "openscad.desktop",
        "openscad"
    ),
    app!(
        "Science & mathematics",
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
        "Utilities",
        "Pika Backup",
        "org.gnome.World.PikaBackup.desktop",
        "pika-backup"
    ),
    app!(
        "Utilities",
        "Passwords and Keys",
        "org.gnome.seahorse.Application.desktop",
        "seahorse"
    ),
    app!("Utilities", "GNU Privacy Assistant", "gpa.desktop", "gpa"),
    app!(
        "Utilities",
        "Kleopatra",
        "org.kde.kleopatra.desktop",
        "kleopatra"
    ),
    app!(
        "Utilities",
        "Filelight",
        "org.kde.filelight.desktop",
        "filelight"
    ),
    app!("Utilities", "QDirStat", "qdirstat.desktop", "qdirstat"),
    app!("Utilities", "KRename", "org.kde.krename.desktop", "krename"),
    app!("Utilities", "KFind", "org.kde.kfind.desktop", "kfind"),
    app!("Utilities", "Brasero", "brasero.desktop", "brasero"),
    app!("Utilities", "Xfburn", "xfburn.desktop", "xfburn"),
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
            assert!(!app.desktop.is_empty(), "missing desktop id: {}", app.name);
            assert!(
                desktop_ids.insert(app.desktop),
                "duplicate desktop id: {}",
                app.desktop
            );
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
