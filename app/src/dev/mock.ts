// Faux côté Rust pour preview.html : données réalistes, aucune action réelle.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const now = Math.floor(Date.now() / 1000);
const dl = "C:\\Users\\demo\\Downloads";

const folders = ["Bureau", "Téléchargements", "Documents", "Images", "Musique", "Vidéos"].map((name, i) => ({
  name,
  path: ["C:\\Users\\demo\\Desktop", dl, "C:\\Users\\demo\\Documents", "C:\\Users\\demo\\Pictures", "C:\\Users\\demo\\Music", "C:\\Users\\demo\\Videos"][i],
  icon: null,
  total: null,
  free: null,
}));

function entries(path: string) {
  if (path === dl) {
    return [
      { name: "Cybersécurité.rar", path: `${dl}\\Cybersécurité.rar`, isDir: false, size: 380_000_000, modified: now - 3600, hidden: false },
      { name: "photos-vacances.zip", path: `${dl}\\photos-vacances.zip`, isDir: false, size: 1_240_000_000, modified: now - 86400 * 3, hidden: false },
      { name: "WinSAR Rar", path: `${dl}\\WinSAR Rar`, isDir: true, size: 0, modified: now - 600, hidden: false },
      { name: "Wifi Hacking - Le Guide Complet.pdf", path: `${dl}\\Wifi Hacking - Le Guide Complet.pdf`, isDir: false, size: 255_000, modified: now - 86400 * 700, hidden: false },
      { name: "setup.exe", path: `${dl}\\setup.exe`, isDir: false, size: 48_000_000, modified: now - 86400 * 20, hidden: false },
      { name: "desktop.ini", path: `${dl}\\desktop.ini`, isDir: false, size: 282, modified: now - 86400 * 900, hidden: true },
      ...Array.from({ length: 60 }, (_, i) => ({
        name: `rapport-${String(i + 1).padStart(3, "0")}.docx`,
        path: `${dl}\\rapport-${i + 1}.docx`,
        isDir: false,
        size: 20_000 + i * 1234,
        modified: now - i * 7200,
        hidden: false,
      })),
    ];
  }
  return [{ name: "exemple.txt", path: `${path}\\exemple.txt`, isDir: false, size: 1200, modified: now, hidden: false }];
}

const archive = {
  path: `${dl}\\Cybersécurité.rar`,
  name: "Cybersécurité.rar",
  format: "rar",
  formatLabel: "RAR",
  size: 380_000_000,
  volumes: 1,
  entries: [
    "9781788622059-LEARN_ETHICAL_HACKING_FROM_SCRATCH.pdf",
    "9781789611809-LEARN_KALI_LINUX_2019.pdf",
    "Beginning Ethical Hacking with Kali Linux.pdf",
    "Linux Basics For Hackers.pdf",
    "Python and Hacking.pdf",
    "techniques-de-hacking-de-jon-erikson.pdf",
    "Wifi Hacking - Le Guide Complet.pdf",
  ].map((p, i) => ({ path: p, size: 30_000_000 + i * 9_000_000, packedSize: 28_000_000 + i * 8_000_000, isDir: false, modified: now - 86400 * (400 + i * 30), encrypted: false }))
    .concat([
      { path: "Outils.zip", size: 2_400_000, packedSize: 2_390_000, isDir: false, modified: now - 86400 * 12, encrypted: false },
      { path: "lisez-moi.txt", size: 1_800, packedSize: 900, isDir: false, modified: now - 86400 * 12, encrypted: false },
    ]),
};

const nested = {
  path: "C:\\Users\\demo\\AppData\\Local\\Temp\\WinSAR\\42-0\\Outils.zip",
  name: "Outils.zip",
  format: "zip",
  formatLabel: "ZIP",
  size: 2_390_000,
  volumes: 1,
  entries: ["nmap/nmap.exe", "nmap/README.md", "wireshark-portable.exe", "logo.png"].map((p, i) => ({
    path: p,
    size: 400_000 + i * 200_000,
    packedSize: 380_000 + i * 190_000,
    isDir: false,
    modified: now - 86400 * 30,
    encrypted: false,
  })),
};

const temp = "C:\\Users\\demo\\AppData\\Local\\Temp\\WinSAR\\42-0";
const logo = "data:image/svg+xml;base64," + btoa('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#4F46E5"/><rect x="18" y="18" width="28" height="28" rx="6" fill="#C7C3FF"/></svg>');

mockWindows("main");
mockIPC(
  (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "launch_info":
        return { action: "open", files: [], format: null };
      case "formats":
        return [
          ["zip", "ZIP", "zip", true, true],
          ["seven-z", "7-Zip", "7z", true, true],
          ["rar", "RAR", "rar", false, false],
          ["tar", "TAR", "tar", true, false],
          ["tar-gz", "TAR.GZ", "tar.gz", true, false],
          ["tar-xz", "TAR.XZ", "tar.xz", true, false],
          ["tar-zst", "TAR.ZST", "tar.zst", true, false],
        ].map(([id, label, extension, canCreate, canEncrypt]) => ({ id, label, extension, canCreate, canEncrypt }));
      case "places":
        return {
          folders,
          drives: [
            { name: "Disque local (C:)", path: "C:\\", icon: null, total: 1_000_000_000_000, free: 312_000_000_000 },
            { name: "Données (D:)", path: "D:\\", icon: null, total: 2_000_000_000_000, free: 120_000_000_000 },
          ],
          home: "C:\\Users\\demo",
        };
      case "recent_list":
        return [
          { path: archive.path, name: archive.name, size: archive.size, opened: now - 60 },
          { path: `${dl}\\photos-vacances.zip`, name: "photos-vacances.zip", size: 1_240_000_000, opened: now - 3600 },
        ];
      case "list_dir":
        return entries(String(a.path));
      case "path_kind":
        return String(a.path).match(/\.(rar|zip|7z)$/i) ? "file" : "dir";
      case "open_archive":
        return String(a.path).endsWith("Outils.zip") ? nested : archive;
      case "materialize_entries":
        return ((a.req as { items: string[] }).items).map((i) => `${temp}\\${i.split("/").pop()}`);
      case "peek": {
        const p = String(a.path);
        if (/\.(png|svg)$/i.test(p)) return { kind: "image", url: logo };
        if (/\.(txt|md)$/i.test(p)) return { kind: "text", text: "Lisez-moi\n=========\n\nCette archive contient les guides de la formation.\nBonne lecture !\n", truncated: false };
        return { kind: "other" };
      }
      case "hash_files":
        return ((a.req as { paths: string[] }).paths).map((p, i) => ({
          path: p,
          size: 1_800 + i * 1000,
          crc32: (0x352441c2 + i).toString(16),
          sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".slice(0, 60) + String(1000 + i),
        }));
      case "settings_load":
        return null;
      case "plugin:app|version":
        return "0.1.0";
      case "system_accent":
        return { light: "#9a1f88", dark: "#f27ee0" };
      case "file_icons":
      case "path_icons":
        return {};
      case "file_types_status":
        return Object.fromEntries((a.exts as string[]).map((e, i) => [e, { registered: i < 3, default: i === 0, current: i === 1 ? "WinRAR" : null }]));
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

await import("../main");
