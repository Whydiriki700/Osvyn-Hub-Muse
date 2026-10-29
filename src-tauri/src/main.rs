#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Osvyn v0.1.4 (29 Okt 2026, ACC mockup Hub oleh abang): Hub — layar
// pusat semua profil (webview "hub", hanya terbuka untuk tingkat Hub) +
// gerbang tingkat ditegakkan inti Rust: batas profil per paket (Founding
// 3 / Personal 6 / Bundle Studio 10 / Hub tanpa batas) berlaku di
// tambah_profil, daftar_profil, dan pembuatan webview saat setup; nama
// paket aktif tampil di bar; kontrol lanjut (Tidurkan semua) untuk
// Bundle Studio ke atas. Status profil untuk Hub dicatat di konfig
// (kolom "terakhir": kapan profil dibuka).
//
// Muse Desktop v0.3.7 — 1 aplikasi, tab profil dinamis, lepas dari Chrome.
// v0.3.7 (28 Okt 2026, perintah abang): Spark Intro diperpanjang jadi
// total 5 detik (sebelumnya 2,45 detik — terlalu mepet, feel-nya tidak
// terasa); DeepSleep — profil yang nonaktif terus selama 3 menit
// halamannya dilepas ke about:blank supaya RAM WebView2-nya bebas,
// klik tabnya memuat ulang muse.ai (login aman, cookie tetap di folder
// data profil). Background-throttling bawaan Tauri TIDAK didukung di
// Windows, jadi pelepasan halaman adalah jalur suspend yang tersedia.
// v0.3.6 (28 Okt 2026, paket fitur terkunci abang): Spark Intro — animasi
// logo Muse di background putih bersih saat aplikasi dibuka (klik = skip);
// Shift — sesudah tambah profil, aplikasi menyala ulang dan membuka dengan
// splash "Menyiapkan profil baru" supaya restart terbaca sebagai transisi
// yang disengaja, bukan error.
//
// Revisi abang 28 Okt 2026 (dari v0.1.0):
// 1. Profil TIDAK dibatasi 3 — ada tombol "+" di bar untuk menambah profil
//    baru kapan pun; nama profil diisi abang saat menambah.
// 2. Label tab tidak terkunci ke nama bawaan: identitas profil = id stabil
//    (folder datanya tetap), nama tampil bisa diganti (klik dua kali tab)
//    sehingga tambah/ganti nama tidak pernah merusak data login.
// 3. Teks "3 profil jalan bersamaan" di kanan bar DIHAPUS — titik hijau di
//    tiap tab sudah cukup.
// 4. (v0.3.0) Ada pilihan HAPUS tab: tombol x di tab, pakai konfirmasi
//    karena folder data (login) profil itu ikut terhapus; minimal sisa 1.
//    Kotak tambah profil juga punya tombol "Buat" (alur full mouse).
//
// Semua profil: webview sendiri, folder data (cookie/login) terpisah,
// dibuat sekaligus dan tetap hidup bersamaan; pindah tab = show/hide saja.
// Daftar profil tersimpan di profiles.json di folder data aplikasi.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{DownloadEvent, NewWindowResponse, Webview, WebviewBuilder};
use tauri::window::WindowBuilder;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, RunEvent, State, Window, WindowEvent};
use tauri::WebviewUrl;

const LOG: &str = r"C:\NexusCorp\osvyn\osvyn_app.log";
const BAR_H: f64 = 48.0;
// DeepSleep: profil nonaktif selama ini (detik) halamannya dilepas.
const TIDUR_SESUDAH_DETIK: u64 = 180;

fn def_persen() -> i64 {
    -1
}

#[derive(Clone, Serialize, Deserialize)]
struct ProfilCfg {
    id: String,
    nama: String,
    // Kolom Hub eksklusif (v0.2.0): semua #[serde(default)] agar
    // profiles.json lama terbaca apa adanya (migrasi tanpa sentuhan).
    #[serde(default)]
    grup: String,
    #[serde(default)]
    catatan: String,
    // Detik unix reset limit berikutnya (diatur pemilik); 0 = belum diatur.
    #[serde(default)]
    reset_unix: i64,
    // Persen pemakaian limit yang diisi manual pemilik; -1 = belum diisi.
    #[serde(default = "def_persen")]
    persen: i64,
    // Profil sementara: folder datanya dibersihkan setiap aplikasi mulai.
    #[serde(default)]
    sementara: bool,
    // Avatar profil untuk Hub (perintah abang 29 Okt 16:32): data URL
    // gambar kecil hasil proses hub.html (maks 128px); kosong = inisial.
    #[serde(default)]
    avatar: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct JadwalGrup {
    // "HH:MM" waktu setempat (WIB); string kosong = tidak dijadwalkan.
    #[serde(default)]
    bangun: String,
    #[serde(default)]
    tidur: String,
}

#[derive(Serialize, Deserialize)]
struct Konfig {
    berikut: u32,
    profil: Vec<ProfilCfg>,
    #[serde(default)]
    aktif: Option<String>,
    // Terakhir kali tiap profil dibuka (detik unix, untuk status Hub).
    // Nilai 0 = profil lama yang pernah dibuka sebelum pencatatan ada
    // (migrasi v0.1.4); tidak ada entri = belum pernah dibuka.
    #[serde(default)]
    terakhir: HashMap<String, i64>,
    // Jadwal rutinitas per nama grup (Hub eksklusif v0.2.0).
    #[serde(default)]
    jadwal: HashMap<String, JadwalGrup>,
}

struct AppState {
    konfig: Mutex<Konfig>,
    basis: PathBuf,
    // DeepSleep: kapan tiap profil mulai nonaktif + siapa yang sedang tidur.
    nonaktif_sejak: Mutex<HashMap<String, Instant>>,
    tidur: Mutex<HashSet<String>>,
    // Gerbang tingkat (v0.1.4): tingkat lisensi aktif + batas profilnya.
    // tingkat 0 = belum ada lisensi (jalur aktivasi, gerbang tidak dipakai).
    tingkat: u8,
    batas: usize,
    // Split view Hub (v0.2.0): id profil pendamping yang tampil
    // berdampingan dengan profil aktif; None = tampilan tunggal biasa.
    split: Mutex<Option<String>>,
    // Berkas unduhan terakhir yang berhasil (untuk toast bar yang bisa
    // diklik -> tampilkan di Windows Explorer).
    unduhan_terakhir: Mutex<Option<PathBuf>>,
}

fn cap_waktu() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 7 * 3600;
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m, d,
        tod / 3600,
        (tod / 60) % 60,
        tod % 60
    )
}

fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG) {
        let _ = writeln!(f, "[{}] {}", cap_waktu(), msg);
    }
}

fn tampil_jendela(app: &AppHandle) {
    if let Some(w) = app.get_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn sembunyi_jendela(app: &AppHandle) {
    if let Some(w) = app.get_window("main") {
        let _ = w.hide();
    }
}

fn keluar(app: &AppHandle) {
    log("keluar via tray: aplikasi berhenti");
    app.exit(0);
}

fn berkas_konfig(basis: &Path) -> PathBuf {
    basis.join("profiles.json")
}

fn muat_konfig(basis: &Path) -> Konfig {
    let f = berkas_konfig(basis);
    if let Ok(teks) = std::fs::read_to_string(&f) {
        if let Ok(k) = serde_json::from_str::<Konfig>(&teks) {
            if !k.profil.is_empty() {
                return k;
            }
        }
    }
    // Benih awal versi jualan: satu profil kosong. Pembeli menambah dan
    // menamai profilnya sendiri sesudah aktivasi.
    Konfig {
        berikut: 2,
        aktif: Some("profil-1".into()),
        profil: vec![ProfilCfg {
            id: "profil-1".into(),
            nama: "Profile 1".into(),
            grup: String::new(),
            catatan: String::new(),
            reset_unix: 0,
            persen: -1,
            sementara: false,
            avatar: String::new(),
        }],
        terakhir: HashMap::new(),
        jadwal: HashMap::new(),
    }
}

fn simpan_konfig(state: &AppState) {
    if let Ok(k) = state.konfig.lock() {
        if let Ok(teks) = serde_json::to_string_pretty(&*k) {
            let _ = std::fs::write(berkas_konfig(&state.basis), teks);
        }
    }
}

// ================= Lisensi Osvyn (versi jualan) =================
// Status lisensi ditentukan inti Rust, bukan GUI (Rencana Keamanan
// Lapis 1-2). MuseKey = base32(payload 13 byte + tanda tangan Ed25519
// 64 byte). Payload: [versi][tingkat][hari u16 LE][bulan update][serial u64 LE].
// Kunci publik tertanam di sini; kunci privat hanya ada di alat penerbit
// penjual dan tidak pernah masuk binary.

const KUNCI_PUBLIK_HEX: &str = "fe1e219cf48ed51a1382da8c0ae37a5ef2a30f78392e7b5a7c8fa7cdd83e00be";

// Server lisensi (Tahap 2, Rencana Keamanan Lapis 5): aplikasi melapor
// sesudah aktivasi dan memeriksa status berkala. Server membalas struk
// bertanda tangan Ed25519 dengan kunci server yang publiknya tertanam di
// sini; status "cabut" mengunci aplikasi lagi, status "aktif" memberi
// masa tenggang offline 7 hari. Anon key memang kunci publik Supabase
// (aman tertanam); tabel server terkunci RLS dan hanya fungsi server
// bertanda tangan yang bisa menjawab.
const URL_FUNGSI_SERVER: &str =
    "https://wvsjvvucoxchuauqjkut.supabase.co/functions/v1";
const ANON_SERVER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Ind2c2p2dnVjb3hjaHVhdXFqa3V0Iiwicm9sZSI6ImFub24iLCJpYXQiOjE3OTMxNjM5MjUsImV4cCI6MjEwODczOTkyNX0.iTDzpOxLgQCRiGqp6__Wn4VmrOhxJzVmVNs0Hz8vw04";
const KUNCI_PUBLIK_SERVER_HEX: &str =
    "17c2b626db802b8352b378ee3bc20ee1dfd158746d41653ae956b0a8b413bdec";

#[derive(Clone, Serialize, Deserialize)]
struct Lisensi {
    kunci: String,
    tingkat: u8,
    serial: u64,
    // Struk pemeriksaan server terakhir (kosong pada berkas lisensi lama
    // dari sebelum v0.1.3; terisi pada pemeriksaan pertama).
    #[serde(default)]
    struk_status: String,
    #[serde(default)]
    struk_ok_sampai: i64,
    #[serde(default)]
    struk_tanda_tangan: String,
    #[serde(default)]
    cek_terakhir: i64,
}

fn sekarang_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn nama_komputer() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string())
}

// Identitas mesin: SHA-256 dari MachineGuid Windows + nama komputer.
// Cukup stabil untuk ikatan lisensi dan tidak membocorkan nilai aslinya.
fn id_mesin() -> String {
    use sha2::Digest;
    let mut guid = String::new();
    let mut perintah_reg = std::process::Command::new("reg");
    // CREATE_NO_WINDOW: reg.exe adalah aplikasi console; tanpa flag ini
    // Windows memunculkan jendela CMD sekedip setiap id_mesin() dipanggil
    // (saat startup untuk cek lisensi, dan saat aplikasi menyala ulang
    // ketika membuat profil baru). Laporan abang 29 Okt 17:06: jendela
    // itu tidak perlu diperlihatkan di frontend.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        perintah_reg.creation_flags(0x0800_0000);
    }
    if let Ok(out) = perintah_reg
        .args([
            "query",
            "HKLM\\SOFTWARE\\Microsoft\\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .output()
    {
        let teks = String::from_utf8_lossy(&out.stdout);
        for baris in teks.lines() {
            if let Some(pos) = baris.find("REG_SZ") {
                guid = baris[pos + 6..].trim().to_string();
            }
        }
    }
    if guid.is_empty() {
        guid = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default();
    }
    let bahan = format!("{}|{}", guid, nama_komputer());
    let hasil = sha2::Sha256::digest(bahan.as_bytes());
    hasil.iter().map(|b| format!("{:02x}", b)).collect()
}

fn verifikasi_struk(
    serial: u64,
    id_mesin: &str,
    status: &str,
    ok_sampai: i64,
    tanda_hex: &str,
) -> bool {
    use ed25519_dalek::Verifier;
    let pk_byte = hex_ke_byte(KUNCI_PUBLIK_SERVER_HEX);
    let pk_arr: &[u8; 32] = match pk_byte.as_slice().try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let pk = match ed25519_dalek::VerifyingKey::from_bytes(pk_arr) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig_byte = hex_ke_byte(tanda_hex);
    let sig_arr: &[u8; 64] = match sig_byte.as_slice().try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let tanda = ed25519_dalek::Signature::from_bytes(sig_arr);
    let pesan = format!("OSVYN-CEK|{}|{}|{}|{}", serial, id_mesin, status, ok_sampai);
    pk.verify(pesan.as_bytes(), &tanda).is_ok()
}

// Panggil fungsi server; None berarti server tidak terjangkau (offline),
// Some berarti ada jawaban apa pun termasuk penolakan berkode HTTP.
fn panggil_server(path: &str, body: &serde_json::Value) -> Option<serde_json::Value> {
    let url = format!("{}/{}", URL_FUNGSI_SERVER, path);
    let hasil = ureq::post(&url)
        .set("apikey", ANON_SERVER)
        .set("Authorization", &format!("Bearer {}", ANON_SERVER))
        .timeout(std::time::Duration::from_secs(8))
        .send_json(body.clone());
    match hasil {
        Ok(r) => r.into_json().ok(),
        Err(ureq::Error::Status(_, r)) => r.into_json().ok(),
        Err(_) => None,
    }
}

fn simpan_lisensi(basis: &Path, lis: &Lisensi) -> Result<(), String> {
    let teks = serde_json::to_string_pretty(lis)
        .map_err(|_| "Failed to save the license.".to_string())?;
    std::fs::write(berkas_lisensi(basis), teks)
        .map_err(|_| "Failed to save the license.".to_string())
}

// Terapkan struk server ke berkas lisensi. Mengembalikan false bila struk
// menyatakan kunci tidak berlaku lagi (cabut / tidak dikenal / batas mesin)
// dan berkas lisensi sudah dihapus.
fn terapkan_struk(basis: &Path, lis: &mut Lisensi, struk: &serde_json::Value) -> bool {
    let status = struk.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let ok_sampai = struk.get("ok_sampai").and_then(|v| v.as_i64()).unwrap_or(0);
    let tanda = struk
        .get("tanda_tangan")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !verifikasi_struk(lis.serial, &id_mesin(), status, ok_sampai, tanda) {
        log("struk server ditolak: tanda tangan tidak sah");
        return true;
    }
    if status != "aktif" {
        let _ = std::fs::remove_file(berkas_lisensi(basis));
        log(&format!(
            "lisensi dimatikan server: serial {} status {}",
            lis.serial, status
        ));
        return false;
    }
    lis.struk_status = status.to_string();
    lis.struk_ok_sampai = ok_sampai;
    lis.struk_tanda_tangan = tanda.to_string();
    lis.cek_terakhir = sekarang_unix();
    let _ = simpan_lisensi(basis, lis);
    true
}

fn nyalakan_ulang(app: &AppHandle) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--tunggu").spawn();
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        app2.exit(0);
    });
}

// Pemeriksaan berkala ke server: 15 detik sesudah start, lalu tiap 24 jam.
// Server tidak terjangkau tidak mengubah apa pun (masa tenggang yang
// berjalan); jawaban "tidak aktif" mengunci aplikasi lewat start ulang.
fn utas_cek_server(app: AppHandle, basis: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(15));
        loop {
            if let Some(mut lis) = status_lisensi_mentah(&basis) {
                let body = serde_json::json!({
                    "serial": lis.serial,
                    "id_mesin": id_mesin(),
                });
                match panggil_server("osvyn-cek", &body) {
                    Some(j) => {
                        if let Some(struk) = j.get("struk") {
                            if !terapkan_struk(&basis, &mut lis, struk) {
                                nyalakan_ulang(&app);
                                return;
                            }
                            log("cek server: lisensi aktif, struk diperbarui");
                        }
                    }
                    None => log("cek server: tidak terjangkau, masa tenggang berjalan"),
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(24 * 3600));
        }
    });
}

fn berkas_lisensi(basis: &Path) -> PathBuf {
    basis.join("lisensi.json")
}

fn hex_ke_byte(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn base32_ke_byte(teks: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut nbit: u32 = 0;
    for c in teks.chars() {
        if c == '=' {
            break;
        }
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            '2'..='7' => c as u32 - '2' as u32 + 26,
            _ => return None,
        };
        buf = (buf << 5) | v;
        nbit += 5;
        if nbit >= 8 {
            nbit -= 8;
            out.push((buf >> nbit) as u8);
            buf &= (1 << nbit) - 1;
        }
    }
    Some(out)
}

fn verifikasi_kunci(teks: &str) -> Result<Lisensi, String> {
    let bersih: String = teks
        .trim()
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let bersih = bersih.strip_prefix("OSV1").unwrap_or(&bersih).to_string();
    let blob = base32_ke_byte(&bersih).ok_or_else(|| "Unknown key format.".to_string())?;
    if blob.len() != 77 {
        return Err("Unknown key format.".to_string());
    }
    let (payload, sig) = blob.split_at(13);
    if payload[0] != 1 {
        return Err("Unsupported key version.".to_string());
    }
    use ed25519_dalek::Verifier;
    let pk_byte = hex_ke_byte(KUNCI_PUBLIK_HEX);
    let pk_arr: &[u8; 32] = pk_byte
        .as_slice()
        .try_into()
        .map_err(|_| "Internal key error.".to_string())?;
    let pk = ed25519_dalek::VerifyingKey::from_bytes(pk_arr)
        .map_err(|_| "Internal key error.".to_string())?;
    let sig_arr: &[u8; 64] = sig
        .try_into()
        .map_err(|_| "Unknown key format.".to_string())?;
    let tanda = ed25519_dalek::Signature::from_bytes(sig_arr);
    pk.verify(payload, &tanda)
        .map_err(|_| "Invalid key. Double-check the key from your purchase email.".to_string())?;
    let tingkat = payload[1];
    let serial = u64::from_le_bytes(payload[5..13].try_into().unwrap());
    Ok(Lisensi {
        kunci: teks.trim().to_string(),
        tingkat,
        serial,
        struk_status: String::new(),
        struk_ok_sampai: 0,
        struk_tanda_tangan: String::new(),
        cek_terakhir: 0,
    })
}

// Baca berkas lisensi + buktikan tanda tangan kuncinya, TANPA menilai
// struk server. Dipakai utas pemeriksaan (yang justru bertugas mengambil
// struk baru) dan oleh status_lisensi.
fn status_lisensi_mentah(basis: &Path) -> Option<Lisensi> {
    let teks = std::fs::read_to_string(berkas_lisensi(basis)).ok()?;
    let l: Lisensi = serde_json::from_str(&teks).ok()?;
    match verifikasi_kunci(&l.kunci) {
        Ok(v) if v.serial == l.serial && v.tingkat == l.tingkat => Some(l),
        _ => None,
    }
}

fn status_lisensi(basis: &Path) -> Option<Lisensi> {
    let l = status_lisensi_mentah(basis)?;
    // Bila struk server sudah pernah diterima, struk itu ikut menentukan:
    // status bukan aktif, tanda tangan rusak, atau masa tenggang offline
    // 7 hari lewat berarti terkunci. Berkas lama tanpa struk (dari sebelum
    // v0.1.3) tetap lolos gerbang ini; utas pemeriksaan mengambil struk
    // pertamanya dalam hitungan detik sesudah start.
    if !l.struk_tanda_tangan.is_empty() {
        if l.struk_status != "aktif" {
            return None;
        }
        if !verifikasi_struk(
            l.serial,
            &id_mesin(),
            &l.struk_status,
            l.struk_ok_sampai,
            &l.struk_tanda_tangan,
        ) {
            return None;
        }
        if sekarang_unix() > l.struk_ok_sampai {
            return None;
        }
    }
    Some(l)
}

fn nama_tingkat(t: u8) -> &'static str {
    match t {
        1 => "Founding",
        2 => "Personal",
        3 => "Bundle Studio",
        4 => "Hub",
        _ => "Tidak dikenal",
    }
}

// Batas profil per tingkat (matriks paket final, ACC abang 29 Okt 2026).
fn batas_profil(t: u8) -> usize {
    match t {
        1 => 3,
        2 => 6,
        3 => 10,
        4 => usize::MAX,
        _ => 3,
    }
}

#[derive(Serialize)]
struct InfoPaket {
    tingkat: u8,
    nama: String,
    serial: u64,
    batas: Option<usize>,
    terpakai: usize,
    fitur_lanjut: bool,
    hub: bool,
}

#[derive(Serialize)]
struct DataHub {
    id: String,
    nama: String,
    // "dibuka" (sedang tampil) | "tidur" (DeepSleep) | "aktif" (pernah
    // dibuka) | "belum" (belum pernah dibuka)
    status: String,
    // Detik unix terakhir dibuka; 0 = pernah dibuka sebelum pencatatan
    // ada; -1 = belum pernah dibuka.
    terakhir: i64,
    // Kolom Hub eksklusif v0.2.0 (dari ProfilCfg).
    grup: String,
    catatan: String,
    reset_unix: i64,
    persen: i64,
    sementara: bool,
    avatar: String,
    // true bila profil ini sedang tampil sebagai pendamping split view.
    pendamping: bool,
}

#[tauri::command]
fn aktivasi_kunci(
    app: AppHandle,
    state: State<'_, AppState>,
    kunci: String,
) -> Result<String, String> {
    let mut lis = verifikasi_kunci(&kunci)?;
    // Lapor ke server lisensi lebih dulu (Tahap 2): kunci yang dicabut,
    // tidak terdaftar, atau sudah penuh 3 mesin ditolak di sini walau
    // tanda tangan lokalnya sah. Server tidak terjangkau (offline) tidak
    // menggagalkan aktivasi; pemeriksaan berkala menyusul sesudah start.
    let mesin = id_mesin();
    let body = serde_json::json!({
        "serial": lis.serial,
        "tingkat": lis.tingkat,
        "id_mesin": mesin,
        "label": nama_komputer(),
        "versi_app": env!("CARGO_PKG_VERSION"),
    });
    match panggil_server("osvyn-aktivasi", &body) {
        Some(j) => {
            if let Some(struk) = j.get("struk") {
                let st = struk.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if st != "aktif" || !terapkan_struk(&state.basis, &mut lis, struk) {
                    return Err(
                        "This key has been revoked. Please contact the seller.".to_string(),
                    );
                }
            } else {
                match j.get("alasan").and_then(|v| v.as_str()) {
                    Some("serial_tidak_dikenal") => {
                        return Err(
                            "This key is not registered. Please contact the seller."
                                .to_string(),
                        );
                    }
                    Some("batas_mesin") => {
                        return Err(
                            "This key is already active on 3 computers.".to_string(),
                        );
                    }
                    Some("dicabut") => {
                        return Err(
                            "This key has been revoked. Please contact the seller."
                                .to_string(),
                        );
                    }
                    _ => {}
                }
            }
        }
        None => log("aktivasi: server tidak terjangkau, lanjut offline"),
    }
    if lis.struk_tanda_tangan.is_empty() {
        simpan_lisensi(&state.basis, &lis)?;
    }
    log(&format!(
        "lisensi AKTIF: tingkat {} serial {}",
        nama_tingkat(lis.tingkat),
        lis.serial
    ));
    // Sama seperti tambah profil: keadaan penuh hanya dibangun di setup(),
    // jadi aplikasi menyala ulang sendiri sesudah aktivasi berhasil.
    nyalakan_ulang(&app);
    Ok(nama_tingkat(lis.tingkat).to_string())
}

// Bahasa antarmuka (perintah abang 28 Okt 23:37): bawaan Inggris, pilihan
// disimpan di bahasa.json di folder data aplikasi supaya layar aktivasi,
// bar profil, dan splash memakai bahasa yang sama.
#[tauri::command]
fn ambil_bahasa(state: State<'_, AppState>) -> String {
    let p = state.basis.join("bahasa.json");
    if let Ok(teks) = std::fs::read_to_string(&p) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&teks) {
            if let Some(k) = v.get("kode").and_then(|x| x.as_str()) {
                return k.to_string();
            }
        }
    }
    "en".to_string()
}

#[tauri::command]
fn simpan_bahasa(state: State<'_, AppState>, kode: String) -> Result<(), String> {
    let aman: String = kode
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_lowercase();
    let isi = format!("{{\"kode\":\"{}\"}}", aman);
    std::fs::write(state.basis.join("bahasa.json"), isi)
        .map_err(|_| "Failed to save the language.".to_string())
}


// --- Unduhan (v0.2.2) ---------------------------------------------------

// Dekode base64 standar (dengan padding), tanpa dependensi tambahan.
fn dekode_b64(s: &str) -> Option<Vec<u8>> {
    fn nilai(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bersih: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if bersih.is_empty() || bersih.len() % 4 != 0 {
        return None;
    }
    let mut hasil = Vec::with_capacity(bersih.len() / 4 * 3);
    for potong in bersih.chunks(4) {
        let mut v = [0u32; 4];
        let mut pad = 0;
        for (i, c) in potong.iter().enumerate() {
            if *c == b'=' {
                pad += 1;
                v[i] = 0;
            } else {
                v[i] = nilai(*c)?;
            }
        }
        let n = (v[0] << 18) | (v[1] << 12) | (v[2] << 6) | v[3];
        hasil.push((n >> 16) as u8);
        if pad < 2 {
            hasil.push((n >> 8) as u8);
        }
        if pad < 1 {
            hasil.push(n as u8);
        }
    }
    Some(hasil)
}

// Nama berkas aman untuk ditulis ke disk: buang karakter terlarang
// Windows, karakter kontrol, dan awalan titik/spasi.
fn nama_berkas_aman(mentah: &str) -> String {
    let nama: String = mentah
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .trim()
        .to_string();
    if nama.is_empty() {
        "unduhan".to_string()
    } else {
        nama
    }
}

// Tujuan unduhan unik di folder tujuan: "nama (1).ext", "nama (2).ext", ...
fn tujuan_unduhan_unik(dir: &Path, nama: &str) -> PathBuf {
    let mut tujuan = dir.join(nama);
    let mut n = 1;
    while tujuan.exists() {
        let (batang, ext) = match nama.rsplit_once('.') {
            Some((b, e)) if !b.is_empty() => (b.to_string(), format!(".{}", e)),
            _ => (nama.to_string(), String::new()),
        };
        tujuan = dir.join(format!("{} ({}){}", batang, n, ext));
        n += 1;
    }
    tujuan
}

// Urai %XX pada potongan URL (nama berkas dari alamat unduhan http).
fn urai_persen(s: &str) -> String {
    let b = s.as_bytes();
    let mut hasil: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                hasil.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        hasil.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&hasil).to_string()
}

// Kabar unduhan ke bar profil: toast teks lewat fungsi __toastUnduhan
// yang disediakan index.html (elemen status bar).
fn kabar_unduhan(app: &AppHandle, teks: &str) {
    if let Some(bar) = app.get_webview("bar") {
        let arg = serde_json::to_string(teks).unwrap_or_else(|_| "\"\"".to_string());
        let _ = bar.eval(format!(
            "window.__toastUnduhan && window.__toastUnduhan({})",
            arg
        ));
    }
}

// Jembatan unduhan blob (script injeksi src/jembatan_unduhan.js):
// halaman muse.ai mengirim isi blob lampiran ke perintah ini untuk
// ditulis ke folder Downloads Windows, karena jalur unduhan native
// WebView2 tidak pernah menyelesaikan unduhan blob (bukti log aplikasi
// 29 Okt 17:00 pada v0.2.1: 8x "unduh mulai", nol "unduh selesai",
// nol berkas di Downloads).
#[tauri::command]
fn simpan_unduhan_blob(app: AppHandle, nama: String, isi_b64: String) -> Result<String, String> {
    let data = dekode_b64(&isi_b64).ok_or_else(|| "Data unduhan rusak.".to_string())?;
    if data.len() > 64 * 1024 * 1024 {
        return Err("Berkas terlalu besar.".to_string());
    }
    let nama = nama_berkas_aman(&nama);
    let dir = std::env::var_os("USERPROFILE")
        .map(|u| PathBuf::from(u).join("Downloads"))
        .ok_or_else(|| "Folder Downloads tidak ditemukan.".to_string())?;
    let _ = std::fs::create_dir_all(&dir);
    let tujuan = tujuan_unduhan_unik(&dir, &nama);
    std::fs::write(&tujuan, &data).map_err(|e| format!("Gagal menulis berkas: {}", e))?;
    log(&format!(
        "unduh blob selesai: {} ({} byte)",
        tujuan.display(),
        data.len()
    ));
    let nama_tampil = tujuan
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or(nama);
    catat_unduhan_terakhir(&app, &tujuan);
    // Perintah abang: habis unduh harus LANGSUNG direct ke file
    // Windows — Explorer dibuka otomatis menyorot berkasnya, tanpa
    // menunggu klik pada toast.
    tampilkan_di_explorer(&tujuan);
    kabar_unduhan(&app, &format!("Download finished: {}", nama_tampil));
    Ok(tujuan.to_string_lossy().to_string())
}

fn tampilkan_di_explorer(path: &Path) {
    use std::os::windows::process::CommandExt;
    log(&format!("unduhan dibuka di explorer: {}", path.display()));
    // raw_arg: baris perintah harus persis  explorer /select,"<path>"
    // Escaping baku Command::arg membungkus ulang argumen yang berisi
    // spasi/kutip dan Explorer membuang targetnya (v0.2.6/v0.2.7).
    let hasil = std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn();
    if let Err(e) = hasil {
        log(&format!("explorer gagal dibuka: {}", e));
        if let Some(induk) = path.parent() {
            let _ = std::process::Command::new("explorer").arg(induk).spawn();
        }
    }
}

fn catat_unduhan_terakhir(app: &AppHandle, path: &Path) {
    if let Some(s) = app.try_state::<AppState>() {
        if let Ok(mut t) = s.unduhan_terakhir.lock() {
            *t = Some(path.to_path_buf());
        }
    }
}

// Klik pada toast unduhan di bar: tampilkan berkas terakhir yang
// diunduh di Windows Explorer — bukti langsung di file Windows, bukan
// cuma tulisan di aplikasi. Tanpa unduhan tercatat, buka foldernya.
#[tauri::command]
fn buka_lokasi_unduhan(app: AppHandle) -> Result<(), String> {
    let berkas: Option<PathBuf> = app
        .try_state::<AppState>()
        .and_then(|s| s.unduhan_terakhir.lock().ok().and_then(|t| t.clone()));
    let hasil = match berkas {
        Some(p) if p.exists() => {
            tampilkan_di_explorer(&p);
            return Ok(());
        }
        _ => {
            let dir = std::env::var_os("USERPROFILE")
                .map(|u| PathBuf::from(u).join("Downloads"))
                .ok_or_else(|| "Folder Downloads tidak ditemukan.".to_string())?;
            log(&format!(
                "folder unduhan dibuka di explorer: {}",
                dir.display()
            ));
            std::process::Command::new("explorer").arg(&dir).spawn()
        }
    };
    hasil
        .map(|_| ())
        .map_err(|e| format!("Gagal membuka Explorer: {}", e))
}

// Penanda hidup jembatan unduhan: dipanggil script injeksi sekali per
// muatan halaman profil. Bila baris ini tidak muncul di log sesudah
// halaman muse.ai termuat, jalur IPC dari konten remote masih tertutup.
#[tauri::command]
fn catat_jembatan_unduhan(url: String) {
    if url.contains("muse.ai") {
        log(&format!("jembatan unduhan aktif: {}", url));
    }
}

fn buat_webview(
    window: &Window,
    basis: &Path,
    id: &str,
    lebar: f64,
    tinggi: f64,
) -> Result<Webview, String> {
    let dir = basis.join("profiles").join(id);
    let _ = std::fs::create_dir_all(&dir);
    let url = tauri::Url::parse("https://muse.ai/").map_err(|e| e.to_string())?;
    // v0.3.5: GPU DINYALAKAN (perintah abang 28 Okt 17:13) — tanpa
    // --disable-gpu supaya render muse.ai mulus; dulu dimatikan demi
    // hemat RAM warisan pola Orvyn HQ, efeknya animasi patah-patah.
    // Folder unduhan bawaan Windows (%USERPROFILE%\Downloads); cadangan
    // ke folder data profil bila variabel itu tidak ada.
    let unduhan: PathBuf = std::env::var_os("USERPROFILE")
        .map(|u| PathBuf::from(u).join("Downloads"))
        .unwrap_or_else(|| dir.clone());
    let app_t = window.app_handle().clone();
    let b = WebviewBuilder::new(id, WebviewUrl::External(url))
        .data_directory(dir)
        .initialization_script(include_str!("../../src/jembatan_unduhan.js"))
        // Fix native 29 Okt (workflow terbaru: fix dulu, build sekali di
        // akhir). Tiga handler ini sebelumnya absen semua:
        // 1) Drag and drop: matikan handler bawaan Tauri/Wry di Windows
        //    supaya event drop HTML5 halaman muse.ai menerima file.
        .disable_drag_drop_handler()
        // 2) Pop-up: tanpa handler ini Wry menandai request jendela baru
        //    sebagai handled tanpa membuat jendela (window.open untuk
        //    OAuth mati di tempat). Allow = WebView2 membuka pop-up
        //    bawaannya dengan environment/cookie profil yang sama;
        //    skema non-http(s) ditolak.
        .on_new_window(|url, _fitur| {
            let putusan = match url.scheme() {
                "http" | "https" => NewWindowResponse::Allow,
                _ => NewWindowResponse::Deny,
            };
            log(&format!(
                "jendela baru: {} diizinkan={}",
                url,
                matches!(putusan, NewWindowResponse::Allow)
            ));
            putusan
        })
        // 3) Download: arahkan ke folder Downloads dengan nama unik dan
        //    catat mulai/selesainya di log aplikasi; tanpa handler ini
        //    unduhan berjalan tanpa jejak yang bisa diverifikasi.
        .on_download(move |_wv, event| {
            match event {
                DownloadEvent::Requested { url, destination } => {
                    let _ = std::fs::create_dir_all(&unduhan);
                    let nama_mentah = url
                        .path_segments()
                        .and_then(|s| s.last())
                        .unwrap_or("")
                        .to_string();
                    let nama = nama_berkas_aman(&urai_persen(&nama_mentah));
                    let tujuan = tujuan_unduhan_unik(&unduhan, &nama);
                    log(&format!("unduh mulai: {} -> {}", url, tujuan.display()));
                    *destination = tujuan;
                }
                DownloadEvent::Finished {
                    url,
                    path,
                    success,
                } => {
                    log(&format!(
                        "unduh selesai: {} sukses={} path={:?}",
                        url, success, path
                    ));
                    let nama_tampil = path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| url.to_string());
                    if success {
                        if let Some(p) = &path {
                            catat_unduhan_terakhir(&app_t, p);
                            tampilkan_di_explorer(p);
                        }
                        kabar_unduhan(&app_t, &format!("Download finished: {}", nama_tampil));
                    } else {
                        kabar_unduhan(&app_t, &format!("Download failed: {}", nama_tampil));
                    }
                }
                _ => {}
            }
            true
        });
    window
        .add_child(
            b,
            LogicalPosition::new(0.0, BAR_H),
            LogicalSize::new(lebar, (tinggi - BAR_H).max(100.0)),
        )
        .map_err(|e| e.to_string())
}

fn ukuran_jendela(app: &AppHandle) -> (f64, f64) {
    if let Some(w) = app.get_window("main") {
        let skala = w.scale_factor().unwrap_or(1.0);
        if let Ok(sz) = w.inner_size() {
            return (sz.width as f64 / skala, sz.height as f64 / skala);
        }
    }
    (1280.0, 800.0)
}

fn atur_ukuran(app: &AppHandle, lebar: f64, tinggi: f64) {
    if let Some(bar) = app.get_webview("bar") {
        let _ = bar.set_position(LogicalPosition::new(0.0, 0.0));
        let _ = bar.set_size(LogicalSize::new(lebar, BAR_H));
    }
    let h = (tinggi - BAR_H).max(100.0);
    let ids: Vec<String> = match app.try_state::<AppState>() {
        Some(s) => match s.konfig.lock() {
            Ok(k) => k.profil.iter().map(|p| p.id.clone()).collect(),
            Err(_) => vec![],
        },
        None => vec![],
    };
    for id in ids {
        if let Some(wv) = app.get_webview(&id) {
            let _ = wv.set_position(LogicalPosition::new(0.0, BAR_H));
            let _ = wv.set_size(LogicalSize::new(lebar, h));
        }
    }
    atur_split(app);
    if let Some(sp) = app.get_webview("splash") {
        let _ = sp.set_position(LogicalPosition::new(0.0, 0.0));
        let _ = sp.set_size(LogicalSize::new(lebar, tinggi));
    }
    if let Some(ak) = app.get_webview("aktivasi") {
        let _ = ak.set_position(LogicalPosition::new(0.0, 0.0));
        let _ = ak.set_size(LogicalSize::new(lebar, tinggi));
    }
    if let Some(hb) = app.get_webview("hub") {
        let _ = hb.set_position(LogicalPosition::new(0.0, 0.0));
        let _ = hb.set_size(LogicalSize::new(lebar, tinggi));
    }
}

// Tata letak split view Hub (v0.2.0): bila ada profil pendamping, profil
// aktif menempati separuh kiri dan pendamping separuh kanan; tanpa
// pendamping, profil aktif memenuhi area seperti biasa. Dipanggil dari
// atur_ukuran dan aktifkan agar konsisten di semua jalur.
fn atur_split(app: &AppHandle) {
    let (lebar, tinggi) = ukuran_jendela(app);
    let h = (tinggi - BAR_H).max(100.0);
    let (aktif, split) = match app.try_state::<AppState>() {
        Some(s) => {
            let a = match s.konfig.lock() {
                Ok(k) => k.aktif.clone(),
                Err(_) => None,
            };
            let sp = match s.split.lock() {
                Ok(g) => g.clone(),
                Err(_) => None,
            };
            (a, sp)
        }
        None => (None, None),
    };
    if let (Some(a), Some(sp)) = (&aktif, &split) {
        if a != sp {
            let separuh = (lebar / 2.0).max(200.0);
            if let Some(wv) = app.get_webview(a) {
                let _ = wv.set_position(LogicalPosition::new(0.0, BAR_H));
                let _ = wv.set_size(LogicalSize::new(separuh, h));
                let _ = wv.show();
            }
            if let Some(wv) = app.get_webview(sp) {
                let _ = wv.set_position(LogicalPosition::new(separuh, BAR_H));
                let _ = wv.set_size(LogicalSize::new((lebar - separuh).max(200.0), h));
                let _ = wv.show();
            }
            return;
        }
    }
    if let Some(a) = &aktif {
        if let Some(wv) = app.get_webview(a) {
            let _ = wv.set_position(LogicalPosition::new(0.0, BAR_H));
            let _ = wv.set_size(LogicalSize::new(lebar, h));
        }
    }
}

fn aktifkan(app: &AppHandle, id_target: &str) {
    let ids: Vec<String> = match app.try_state::<AppState>() {
        Some(s) => match s.konfig.lock() {
            Ok(k) => k.profil.iter().map(|p| p.id.clone()).collect(),
            Err(_) => vec![],
        },
        None => vec![],
    };
    // DeepSleep: catat siapa yang mulai nonaktif, siapa yang bangun.
    if let Some(s) = app.try_state::<AppState>() {
        if let Ok(mut m) = s.nonaktif_sejak.lock() {
            let kini = Instant::now();
            for id in &ids {
                if id == id_target {
                    m.remove(id);
                } else {
                    m.entry(id.clone()).or_insert(kini);
                }
            }
        }
        // Catatan Hub: profil ini dibuka sekarang.
        if let Ok(mut k) = s.konfig.lock() {
            k.terakhir.insert(id_target.to_string(), sekarang_unix());
        }
        simpan_konfig(&s);
        catat_aktivitas(&s, id_target, "buka");
        let sedang_tidur = match s.tidur.lock() {
            Ok(mut t) => t.remove(id_target),
            Err(_) => false,
        };
        if sedang_tidur {
            if let Some(wv) = app.get_webview(id_target) {
                if let Ok(url) = tauri::Url::parse("https://muse.ai/") {
                    let _ = wv.navigate(url);
                }
            }
            log(&format!(
                "DeepSleep: profil {} dibangunkan (muat ulang muse.ai)",
                id_target
            ));
        }
    }
    for id in ids {
        if let Some(wv) = app.get_webview(&id) {
            if id == id_target {
                let _ = wv.show();
                let _ = wv.set_focus();
                // Animasi masuk halaman profil (fungsi dari jembatan
                // injeksi; halaman yang sedang dimuat ulang tidak
                // memilikinya dan lewati saja dengan aman).
                let _ = wv.eval("window.__pindahMasuk && window.__pindahMasuk()");
            } else {
                let _ = wv.hide();
            }
        }
    }
    atur_split(app);
}

// DeepSleep: utas pekerja — tiap 10 detik, profil yang nonaktif terus
// selama TIDUR_SESUDAH_DETIK halamannya dinavigasi ke about:blank di
// utas utama (pola yang sama dengan utas pengaman splash). Navigasi
// melepaskan DOM/JS muse.ai dari RAM; proses WebView2 + folder data
// (login) tetap hidup, jadi bangun = navigasi balik saja, tanpa perlu
// membuat webview baru (jalur yang terbukti deadlock di v0.3.0-0.3.2).
fn utas_deepsleep(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10));
        let Some(s) = app.try_state::<AppState>() else {
            continue;
        };
        let aktif: Option<String> = match s.konfig.lock() {
            Ok(k) => k.aktif.clone(),
            Err(_) => None,
        };
        let kandidat: Vec<String> = {
            let kini = Instant::now();
            let mut hasil = vec![];
            if let Ok(m) = s.nonaktif_sejak.lock() {
                if let Ok(t) = s.tidur.lock() {
                    for (id, sejak) in m.iter() {
                        if Some(id) != aktif.as_ref()
                            && !t.contains(id)
                            && kini.duration_since(*sejak)
                                >= Duration::from_secs(TIDUR_SESUDAH_DETIK)
                        {
                            hasil.push(id.clone());
                        }
                    }
                }
            }
            hasil
        };
        for id in kandidat {
            if let Ok(mut t) = s.tidur.lock() {
                t.insert(id.clone());
            }
            log(&format!(
                "DeepSleep: profil {} ditidurkan (halaman dilepas, RAM bebas)",
                id
            ));
            let h = app.clone();
            let id2 = id.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(wv) = h.get_webview(&id2) {
                    if let Ok(url) = tauri::Url::parse("about:blank") {
                        let _ = wv.navigate(url);
                    }
                    let _ = wv.hide();
                }
            });
        }
    });
}

fn sembunyikan_splash(app: &AppHandle) {
    if let Some(wv) = app.get_webview("splash") {
        let _ = wv.hide();
    }
}

#[tauri::command]
fn tutup_splash(app: AppHandle) {
    sembunyikan_splash(&app);
    log("splash ditutup (klik/timeout halaman)");
}

#[tauri::command]
fn daftar_profil(state: State<'_, AppState>) -> Vec<ProfilCfg> {
    // Hanya profil di dalam batas paket yang punya webview dan tampil
    // di bar; sisanya tetap tersimpan di konfig, tidak ikut dibangun.
    match state.konfig.lock() {
        Ok(k) => k.profil.iter().take(state.batas).cloned().collect(),
        Err(_) => vec![],
    }
}

#[tauri::command]
fn ganti_profil(app: AppHandle, state: State<'_, AppState>, profil: String) -> Result<(), String> {
    {
        let k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        let pos = k
            .profil
            .iter()
            .position(|p| p.id == profil)
            .ok_or("Profile not found.")?;
        if pos >= state.batas {
            return Err(
                "This profile is outside your plan's profile limit.".into(),
            );
        }
    }
    aktifkan(&app, &profil);
    if let Ok(mut k) = state.konfig.lock() {
        k.aktif = Some(profil.clone());
    }
    simpan_konfig(&state);
    log(&format!("ganti profil -> {}", profil));
    Ok(())
}

#[tauri::command]
fn profil_tidur(state: State<'_, AppState>) -> Vec<String> {
    match state.tidur.lock() {
        Ok(t) => t.iter().cloned().collect(),
        Err(_) => vec![],
    }
}

#[tauri::command]
fn profil_aktif(state: State<'_, AppState>) -> Option<String> {
    match state.konfig.lock() {
        Ok(k) => k
            .aktif
            .clone()
            .filter(|a| k.profil.iter().any(|p| &p.id == a))
            .or_else(|| k.profil.first().map(|p| p.id.clone())),
        Err(_) => None,
    }
}

// ================= Hub + gerbang tingkat (v0.1.4) =================

#[tauri::command]
fn info_paket(state: State<'_, AppState>) -> InfoPaket {
    let terpakai = match state.konfig.lock() {
        Ok(k) => k.profil.len(),
        Err(_) => 0,
    };
    let serial = status_lisensi_mentah(&state.basis)
        .map(|l| l.serial)
        .unwrap_or(0);
    InfoPaket {
        tingkat: state.tingkat,
        nama: nama_tingkat(state.tingkat).to_string(),
        serial,
        batas: if state.batas == usize::MAX {
            None
        } else {
            Some(state.batas)
        },
        terpakai,
        fitur_lanjut: state.tingkat >= 3,
        hub: state.tingkat >= 4,
    }
}

#[tauri::command]
fn data_hub(state: State<'_, AppState>) -> Vec<DataHub> {
    let tidur: HashSet<String> = match state.tidur.lock() {
        Ok(t) => t.iter().cloned().collect(),
        Err(_) => HashSet::new(),
    };
    let split_id: Option<String> = match state.split.lock() {
        Ok(s) => s.clone(),
        Err(_) => None,
    };
    match state.konfig.lock() {
        Ok(k) => k
            .profil
            .iter()
            .take(state.batas)
            .map(|p| {
                let status = if k.aktif.as_deref() == Some(p.id.as_str()) {
                    "dibuka"
                } else if tidur.contains(&p.id) {
                    "tidur"
                } else if k.terakhir.contains_key(&p.id) {
                    "aktif"
                } else {
                    "belum"
                };
                DataHub {
                    id: p.id.clone(),
                    nama: p.nama.clone(),
                    status: status.to_string(),
                    terakhir: k.terakhir.get(&p.id).copied().unwrap_or(-1),
                    grup: p.grup.clone(),
                    catatan: p.catatan.clone(),
                    reset_unix: p.reset_unix,
                    persen: p.persen,
                    sementara: p.sementara,
                    avatar: p.avatar.clone(),
                    pendamping: split_id.as_deref() == Some(p.id.as_str()),
                }
            })
            .collect(),
        Err(_) => vec![],
    }
}

// ================= Log aktivitas Hub (v0.2.0) =================

#[derive(Clone, Serialize, Deserialize)]
struct EntriAktivitas {
    waktu: i64,
    profil: String,
    nama: String,
    aksi: String,
}

fn berkas_aktivitas(basis: &Path) -> PathBuf {
    basis.join("aktivitas.json")
}

fn catat_aktivitas(state: &AppState, id_profil: &str, aksi: &str) {
    let nama = match state.konfig.lock() {
        Ok(k) => k
            .profil
            .iter()
            .find(|p| p.id == id_profil)
            .map(|p| p.nama.clone())
            .unwrap_or_else(|| id_profil.to_string()),
        Err(_) => id_profil.to_string(),
    };
    catat_aktivitas_nama(state, id_profil, &nama, aksi);
}

fn catat_aktivitas_nama(state: &AppState, id_profil: &str, nama: &str, aksi: &str) {
    let f = berkas_aktivitas(&state.basis);
    let mut isi: Vec<EntriAktivitas> = std::fs::read_to_string(&f)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    isi.push(EntriAktivitas {
        waktu: sekarang_unix(),
        profil: id_profil.to_string(),
        nama: nama.to_string(),
        aksi: aksi.to_string(),
    });
    if isi.len() > 200 {
        let potong = isi.len() - 200;
        isi.drain(0..potong);
    }
    if let Ok(teks) = serde_json::to_string_pretty(&isi) {
        let _ = std::fs::write(&f, teks);
    }
}

#[tauri::command]
fn data_aktivitas(state: State<'_, AppState>) -> Vec<EntriAktivitas> {
    let f = berkas_aktivitas(&state.basis);
    let isi: Vec<EntriAktivitas> = std::fs::read_to_string(&f)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    isi.into_iter().rev().take(50).collect()
}

// Simpan kolom Hub sebuah profil (grup, catatan, reset limit, persen
// manual, tanda sementara). Eksklusif tingkat Hub, digerbang di Rust.
#[tauri::command]
fn simpan_meta_profil(
    state: State<'_, AppState>,
    id: String,
    grup: String,
    catatan: String,
    reset_unix: i64,
    persen: i64,
    sementara: bool,
    avatar: String,
) -> Result<(), String> {
    if state.tingkat < 4 {
        return Err("Hub features are available on the Hub plan.".into());
    }
    if avatar.len() > 300_000 || (!avatar.is_empty() && !avatar.starts_with("data:image/")) {
        return Err("Avatar image is invalid or too large.".into());
    }
    {
        let mut k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        let p = k
            .profil
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Profile not found.")?;
        p.grup = grup.trim().to_string();
        p.catatan = catatan.trim().to_string();
        p.reset_unix = reset_unix.max(0);
        p.persen = persen.clamp(-1, 100);
        p.sementara = sementara;
        p.avatar = avatar;
    }
    simpan_konfig(&state);
    Ok(())
}

#[tauri::command]
fn simpan_jadwal(
    state: State<'_, AppState>,
    grup: String,
    bangun: String,
    tidur: String,
) -> Result<(), String> {
    if state.tingkat < 4 {
        return Err("Hub features are available on the Hub plan.".into());
    }
    let grup = grup.trim().to_string();
    if grup.is_empty() {
        return Err("Group name is empty.".into());
    }
    {
        let mut k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        k.jadwal.insert(
            grup,
            JadwalGrup {
                bangun: bangun.trim().to_string(),
                tidur: tidur.trim().to_string(),
            },
        );
    }
    simpan_konfig(&state);
    Ok(())
}

#[tauri::command]
fn data_jadwal(state: State<'_, AppState>) -> HashMap<String, JadwalGrup> {
    match state.konfig.lock() {
        Ok(k) => k.jadwal.clone(),
        Err(_) => HashMap::new(),
    }
}

// Bangunkan satu profil yang tidur (navigasi balik dari about:blank),
// tanpa mengganti profil aktif. Pola sama dengan jalur bangun di
// aktifkan().
fn bangunkan(app: &AppHandle, id: &str) {
    if let Some(s) = app.try_state::<AppState>() {
        let sedang_tidur = match s.tidur.lock() {
            Ok(mut t) => t.remove(id),
            Err(_) => false,
        };
        if sedang_tidur {
            if let Some(wv) = app.get_webview(id) {
                if let Ok(url) = tauri::Url::parse("https://muse.ai/") {
                    let _ = wv.navigate(url);
                }
            }
            log(&format!("grup: profil {} dibangunkan", id));
        }
        if let Ok(mut m) = s.nonaktif_sejak.lock() {
            m.remove(id);
        }
    }
}

// Buka satu grup: bangunkan semua anggotanya yang tidur, lalu tampilkan
// anggota pertama. Eksklusif tingkat Hub.
#[tauri::command]
fn buka_grup(
    app: AppHandle,
    state: State<'_, AppState>,
    grup: String,
) -> Result<(), String> {
    if state.tingkat < 4 {
        return Err("Hub features are available on the Hub plan.".into());
    }
    let anggota: Vec<String> = {
        let k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        k.profil
            .iter()
            .take(state.batas)
            .filter(|p| p.grup == grup)
            .map(|p| p.id.clone())
            .collect()
    };
    if anggota.is_empty() {
        return Err("Group is empty.".into());
    }
    for id in &anggota {
        bangunkan(&app, id);
    }
    let pertama = anggota[0].clone();
    aktifkan(&app, &pertama);
    if let Ok(mut k) = state.konfig.lock() {
        k.aktif = Some(pertama);
    }
    simpan_konfig(&state);
    log(&format!("grup dibuka: {} ({} profil)", grup, anggota.len()));
    Ok(())
}

// Split view Hub (v0.2.0): tampilkan profil pendamping berdampingan
// dengan profil aktif. Eksklusif tingkat Hub.
#[tauri::command]
fn buka_split(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    if state.tingkat < 4 {
        return Err("Hub features are available on the Hub plan.".into());
    }
    let aktif: Option<String> = {
        let k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        let ada = k
            .profil
            .iter()
            .take(state.batas)
            .any(|p| p.id == id);
        if !ada {
            return Err("Profile not found.".into());
        }
        k.aktif.clone()
    };
    let aktif = aktif.ok_or("No active profile.")?;
    if aktif == id {
        return Err("Pick a different profile for the second pane.".into());
    }
    bangunkan(&app, &id);
    if let Ok(mut sp) = state.split.lock() {
        *sp = Some(id.clone());
    }
    aktifkan(&app, &aktif);
    catat_aktivitas(&state, &id, "split");
    log(&format!("split view: {} di samping {}", id, aktif));
    Ok(())
}

#[tauri::command]
fn tutup_split(app: AppHandle, state: State<'_, AppState>) {
    if let Ok(mut sp) = state.split.lock() {
        *sp = None;
    }
    let aktif: Option<String> = match state.konfig.lock() {
        Ok(k) => k.aktif.clone(),
        Err(_) => None,
    };
    if let Some(a) = aktif {
        aktifkan(&app, &a);
    }
}

// Utas rutinitas Hub (v0.2.0): tiap 20 detik, cocokkan jam WIB sekarang
// dengan jadwal bangun/tidur per grup. Sekali per menit per grup per
// jenis (peta "sudah" menahan tembakan ulang di menit yang sama).
// Jadwal hanya berjalan selama aplikasi menyala.
fn utas_jadwal(app: AppHandle) {
    std::thread::spawn(move || {
        let mut sudah: HashMap<String, i64> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(20));
            let Some(s) = app.try_state::<AppState>() else {
                continue;
            };
            if s.tingkat < 4 {
                continue;
            }
            let wib = sekarang_unix() + 7 * 3600;
            let menit_ini = wib.div_euclid(60);
            let hh = wib.rem_euclid(86400) / 3600;
            let mm = wib.rem_euclid(3600) / 60;
            let jam = format!("{:02}:{:02}", hh, mm);
            let (jadwal_snap, profil_snap, aktif_snap) = match s.konfig.lock() {
                Ok(k) => (k.jadwal.clone(), k.profil.clone(), k.aktif.clone()),
                Err(_) => continue,
            };
            for (grup, j) in &jadwal_snap {
                let anggota: Vec<String> = profil_snap
                    .iter()
                    .filter(|p| &p.grup == grup)
                    .map(|p| p.id.clone())
                    .collect();
                if anggota.is_empty() {
                    continue;
                }
                if !j.bangun.is_empty() && j.bangun == jam {
                    let kunci = format!("{}|bangun", grup);
                    if sudah.get(&kunci).copied() != Some(menit_ini) {
                        sudah.insert(kunci, menit_ini);
                        for id in &anggota {
                            bangunkan(&app, id);
                        }
                        log(&format!("jadwal: grup {} dibangunkan {}", grup, jam));
                    }
                }
                if !j.tidur.is_empty() && j.tidur == jam {
                    let kunci = format!("{}|tidur", grup);
                    if sudah.get(&kunci).copied() != Some(menit_ini) {
                        sudah.insert(kunci, menit_ini);
                        for id in &anggota {
                            if aktif_snap.as_deref() != Some(id.as_str()) {
                                tidurkan_sekarang(&app, id);
                            }
                        }
                        log(&format!("jadwal: grup {} ditidurkan {}", grup, jam));
                    }
                }
            }
        }
    });
}

#[tauri::command]
fn buka_hub(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.tingkat < 4 {
        return Err("The Hub is available on the Hub plan.".into());
    }
    if let Some(h) = app.get_webview("hub") {
        let _ = h.show();
        let _ = h.set_focus();
        let _ = h.eval("window.__hubMasuk && window.__hubMasuk()");
    }
    log("hub dibuka");
    Ok(())
}

#[tauri::command]
fn tutup_hub(app: AppHandle) {
    if let Some(h) = app.get_webview("hub") {
        let _ = h.hide();
    }
    log("hub ditutup");
}

// Penanda agar aplikasi membuka Hub lagi sesudah menyala ulang karena
// tambah profil yang dimulai dari halaman Hub.
#[tauri::command]
fn tandai_hub(state: State<'_, AppState>) {
    let _ = std::fs::write(state.basis.join("hub.flag"), "1");
}

// Tidurkan satu profil sekarang juga (jalur yang sama dengan DeepSleep
// otomatis: halaman dilepas ke about:blank di utas utama, login aman).
fn tidurkan_sekarang(app: &AppHandle, id: &str) {
    if let Some(s) = app.try_state::<AppState>() {
        if let Ok(mut t) = s.tidur.lock() {
            t.insert(id.to_string());
        }
        if let Ok(mut m) = s.nonaktif_sejak.lock() {
            m.remove(id);
        }
        catat_aktivitas(&s, id, "tidur");
    }
    log(&format!(
        "DeepSleep manual: profil {} ditidurkan dari Hub/bar",
        id
    ));
    let h = app.clone();
    let id2 = id.to_string();
    let _ = app.run_on_main_thread(move || {
        if let Some(wv) = h.get_webview(&id2) {
            if let Ok(url) = tauri::Url::parse("about:blank") {
                let _ = wv.navigate(url);
            }
            let _ = wv.hide();
        }
    });
}

#[tauri::command]
fn tidurkan_profil(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let (aktif_ini, dibangun) = {
        let k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        let pos = k.profil.iter().position(|p| p.id == id);
        (
            k.aktif.clone(),
            pos.map(|i| i < state.batas).unwrap_or(false),
        )
    };
    if !dibangun {
        return Err("Profile not found.".into());
    }
    // Bila target adalah pendamping split, matikan split lebih dulu agar
    // tata letak kembali tunggal sebelum profilnya ditidurkan.
    if let Ok(mut sp) = state.split.lock() {
        if sp.as_deref() == Some(id.as_str()) {
            *sp = None;
        }
    }
    let sedang_tidur = match state.tidur.lock() {
        Ok(t) => t.contains(&id),
        Err(_) => false,
    };
    if sedang_tidur {
        return Ok(());
    }
    // Profil yang sedang dibuka tidak boleh ditidurkan dari bawah
    // kakinya sendiri: pindah dulu ke profil terbangun lain.
    if aktif_ini.as_deref() == Some(id.as_str()) {
        let pengganti = {
            let k = state.konfig.lock().map_err(|_| "Configuration error.")?;
            let tidur: HashSet<String> = match state.tidur.lock() {
                Ok(t) => t.iter().cloned().collect(),
                Err(_) => HashSet::new(),
            };
            k.profil
                .iter()
                .take(state.batas)
                .find(|p| p.id != id && !tidur.contains(&p.id))
                .map(|p| p.id.clone())
        };
        match pengganti {
            Some(pg) => {
                aktifkan(&app, &pg);
                if let Ok(mut k) = state.konfig.lock() {
                    k.aktif = Some(pg);
                }
                simpan_konfig(&state);
            }
            None => {
                return Err(
                    "Open another profile first, then put this one to sleep.".into(),
                )
            }
        }
    }
    tidurkan_sekarang(&app, &id);
    Ok(())
}

#[tauri::command]
fn tidurkan_semua(
    app: AppHandle,
    webview: tauri::Webview,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Debounce inti: panggilan beruntun <2 detik diabaikan. Log
    // 29 Okt: satu klik efektif diikuti 6-7 panggilan kosong dalam
    // 1-2 detik (spam dari webview pemanggil); tombol bar sudah
    // dijaga, tombol Hub belum, dan debounce ini mematikan spam dari
    // sumber mana pun. Label webview dicatat untuk provenance.
    {
        static TERAKHIR: Mutex<Option<Instant>> = Mutex::new(None);
        if let Ok(mut g) = TERAKHIR.lock() {
            let kini = Instant::now();
            if let Some(sebelum) = *g {
                if kini.duration_since(sebelum) < Duration::from_secs(2) {
                    log(&format!(
                        "tidurkan semua diabaikan (debounce, pemanggil: {})",
                        webview.label()
                    ));
                    return Ok(());
                }
            }
            *g = Some(kini);
        }
    }
    if state.tingkat < 3 {
        return Err(
            "Sleep controls are available on the Bundle Studio plan and above.".into(),
        );
    }
    let target: Vec<String> = {
        let k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        let tidur: HashSet<String> = match state.tidur.lock() {
            Ok(t) => t.iter().cloned().collect(),
            Err(_) => HashSet::new(),
        };
        // Keputusan abang 29 Okt 19:11: Sleep all menidurkan SEMUA
        // profil, termasuk yang sedang dibuka (keluhan: tombol di bar
        // atas terasa glitch karena profil yang dilihat tidak pernah
        // ikut tidur). Profil aktif ditidurkan TERAKHIR supaya semua
        // proses lain selesai dulu sebelum layar yang dilihat kosong.
        let aktif = k.aktif.clone();
        let mut daftar: Vec<String> = k
            .profil
            .iter()
            .take(state.batas)
            .filter(|p| !tidur.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
        daftar.sort_by_key(|id| aktif.as_deref() == Some(id.as_str()));
        daftar
    };
    for id in &target {
        tidurkan_sekarang(&app, id);
    }
    log(&format!(
        "tidurkan semua: {} profil (pemanggil: {})",
        target.len(),
        webview.label()
    ));
    Ok(())
}

#[tauri::command]
fn tambah_profil(
    app: AppHandle,
    state: State<'_, AppState>,
    nama: String,
) -> Result<ProfilCfg, String> {
    let nama = nama.trim().to_string();
    let cfg = {
        let mut k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        // Gerbang tingkat: batas profil ditegakkan di inti Rust, bukan
        // sekadar tombol yang disembunyikan di tampilan.
        if k.profil.len() >= state.batas {
            return Err(format!(
                "Profile limit reached for the {} plan ({} profiles). Upgrade your plan to add more profiles.",
                nama_tingkat(state.tingkat),
                state.batas
            ));
        }
        let nomor = k.berikut;
        k.berikut += 1;
        let cfg = ProfilCfg {
            id: format!("profil-{}", nomor),
            nama: if nama.is_empty() {
                format!("Profile {}", nomor)
            } else {
                nama
            },
            grup: String::new(),
            catatan: String::new(),
            reset_unix: 0,
            persen: -1,
            sementara: false,
            avatar: String::new(),
        };
        k.profil.push(cfg.clone());
        cfg
    };
    {
        let mut k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        k.aktif = Some(cfg.id.clone());
    }
    simpan_konfig(&state);
    catat_aktivitas(&state, &cfg.id, "tambah");
    // Pembuatan webview profil baru TIDAK bisa dilakukan saat aplikasi
    // sedang jalan: add_child untuk environment WebView2 baru membuat
    // thread utama macet permanen (terbukti di uji v0.3.0, v0.3.1, dan
    // v0.3.2 — dari handler IPC maupun dari run_on_main_thread; konfig
    // tersimpan dan proses WebView2 muncul, tapi aplikasi membeku total).
    // Jalur yang terbukti sehat adalah pembuatan di setup(). Maka tambah
    // profil = simpan konfig, lalu aplikasi menyala ulang sendiri:
    // instance baru dipanggil dengan argumen --tunggu (menunggu proses
    // lama benar-benar keluar agar plugin single-instance tidak menelan
    // instance baru), proses lama keluar 300 ms kemudian, dan setup()
    // instance baru membuat webview profil baru + mengaktifkannya.
    // Shift: tinggalkan penanda supaya instance baru membuka dengan
    // splash transisi "Menyiapkan profil baru", bukan Spark Intro biasa.
    let _ = std::fs::write(state.basis.join("shift.flag"), &cfg.nama);
    log(&format!(
        "profil baru: {} ({}) — aplikasi menyala ulang untuk membuatnya",
        cfg.id, cfg.nama
    ));
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--tunggu").spawn();
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        app2.exit(0);
    });
    Ok(cfg)
}

#[tauri::command]
fn ganti_nama(
    state: State<'_, AppState>,
    id: String,
    nama: String,
) -> Result<(), String> {
    let nama = nama.trim().to_string();
    if nama.is_empty() {
        return Err("Name is empty.".into());
    }
    {
        let mut k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        let p = k
            .profil
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Profile not found.")?;
        p.nama = nama.clone();
    }
    simpan_konfig(&state);
    catat_aktivitas(&state, &id, "ganti nama");
    log(&format!("ganti nama {} -> {}", id, nama));
    Ok(())
}

#[tauri::command]
fn hapus_profil(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let (sisa_pertama, nama) = {
        let mut k = state.konfig.lock().map_err(|_| "Configuration error.")?;
        if k.profil.len() <= 1 {
            return Err("The last profile cannot be deleted.".into());
        }
        let pos = k
            .profil
            .iter()
            .position(|p| p.id == id)
            .ok_or("Profile not found.")?;
        let terhapus = k.profil.remove(pos);
        if k.aktif.as_deref() == Some(id.as_str()) {
            k.aktif = Some(k.profil[0].id.clone());
        }
        (k.profil[0].id.clone(), terhapus.nama)
    };
    simpan_konfig(&state);
    catat_aktivitas_nama(&state, &id, &nama, "hapus");
    // DeepSleep: bersihkan catatan profil yang dihapus.
    if let Ok(mut m) = state.nonaktif_sejak.lock() {
        m.remove(&id);
    }
    if let Ok(mut t) = state.tidur.lock() {
        t.remove(&id);
    }
    // Tampilkan profil lain dulu, baru tutup webview target.
    aktifkan(&app, &sisa_pertama);
    if let Some(wv) = app.get_webview(&id) {
        let _ = wv.close();
    }
    log(&format!("profil terhapus: {} ({})", id, nama));
    // Penghapusan folder data berjalan di utas latar (perbaikan respons
    // 28 Okt 23:37): folder WebView2 sering terkunci beberapa detik
    // sesudah webview ditutup, dan menungguinya di dalam handler membuat
    // tombol Hapus terasa macet beberapa detik. Handler kini kembali
    // seketika; utas latar mengulang sampai folder benar-benar hilang.
    let dir = state.basis.join("profiles").join(&id);
    let id_laporan = id.clone();
    std::thread::spawn(move || {
        let mut terhapus_total = false;
        for _ in 0..20 {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => {
                    terhapus_total = true;
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
            }
        }
        log(&format!(
            "data profil {} terhapus_total={}",
            id_laporan, terhapus_total
        ));
    });
    Ok(())
}

fn main() {
    // Instance pengganti sesudah tambah profil dipanggil dengan --tunggu:
    // beri waktu proses lama keluar dulu supaya plugin single-instance
    // tidak menelan instance baru ini.
    if std::env::args().any(|a| a == "--tunggu") {
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }
    log(&format!(
        "=== Osvyn v{} mulai ===",
        env!("CARGO_PKG_VERSION")
    ));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tampil_jendela(app);
        }))
        .invoke_handler(tauri::generate_handler![
            aktivasi_kunci,
            ambil_bahasa,
            simpan_bahasa,
            daftar_profil,
            ganti_profil,
            tambah_profil,
            simpan_unduhan_blob,
            catat_jembatan_unduhan,
            buka_lokasi_unduhan,
            ganti_nama,
            hapus_profil,
            profil_aktif,
            profil_tidur,
            tutup_splash,
            info_paket,
            data_hub,
            buka_hub,
            tutup_hub,
            tandai_hub,
            tidurkan_profil,
            tidurkan_semua,
            data_aktivitas,
            simpan_meta_profil,
            simpan_jadwal,
            data_jadwal,
            buka_grup,
            buka_split,
            tutup_split
        ])
        .setup(|app| {
            log("setup mulai");
            let handle = app.handle().clone();

            let basis: PathBuf = app
                .path()
                .app_local_data_dir()
                .map_err(|e| e.to_string())?;
            let _ = std::fs::create_dir_all(&basis);
            // Shift: penanda dari tambah_profil instance sebelumnya.
            let berkas_shift = basis.join("shift.flag");
            let mode_shift = berkas_shift.exists();
            if mode_shift {
                let _ = std::fs::remove_file(&berkas_shift);
            }
            let mut konfig_awal = muat_konfig(&basis);
            // Profil sementara (Hub v0.2.0): folder data sesi dibersihkan
            // setiap aplikasi mulai, sebelum webview-nya dibangun, jadi
            // profil jenis ini selalu mulai dari keadaan bersih.
            for p in &konfig_awal.profil {
                if p.sementara {
                    let dir = basis.join("profiles").join(&p.id);
                    if dir.exists() {
                        let _ = std::fs::remove_dir_all(&dir);
                        log(&format!(
                            "profil sementara {} dibersihkan saat mulai",
                            p.id
                        ));
                    }
                }
            }
            // Migrasi v0.1.4: profil lama yang belum punya catatan
            // "terakhir dibuka" ditandai pernah dibuka (nilai 0) supaya
            // status Hub tidak menuduh mereka belum login.
            if konfig_awal.terakhir.is_empty() && !konfig_awal.profil.is_empty() {
                for p in &konfig_awal.profil {
                    konfig_awal.terakhir.insert(p.id.clone(), 0);
                }
            }
            // Gerbang tingkat dibaca sekali di awal dari lisensi yang sah.
            let lisensi0 = status_lisensi(&basis);
            let tingkat0 = lisensi0.as_ref().map(|l| l.tingkat).unwrap_or(0);
            let batas0 = if lisensi0.is_some() {
                batas_profil(tingkat0)
            } else {
                usize::MAX
            };
            let daftar_awal = konfig_awal.profil.clone();
            let pertama = konfig_awal
                .aktif
                .clone()
                .filter(|a| konfig_awal.profil.iter().any(|p| &p.id == a))
                .unwrap_or_else(|| {
                    daftar_awal.first().map(|p| p.id.clone()).unwrap_or_default()
                });
            // Hanya profil di dalam batas paket yang dibangun webview-nya;
            // profil di luar batas tetap tersimpan utuh di konfig.
            let daftar_bangun: Vec<ProfilCfg> =
                daftar_awal.iter().take(batas0).cloned().collect();
            let pertama = if daftar_bangun.iter().any(|p| p.id == pertama) {
                pertama
            } else {
                daftar_bangun
                    .first()
                    .map(|p| p.id.clone())
                    .unwrap_or_default()
            };
            // DeepSleep: semua profil selain yang pertama aktif mulai
            // dihitung nonaktif sejak aplikasi menyala.
            let mut peta_nonaktif: HashMap<String, Instant> = HashMap::new();
            for p in &daftar_awal {
                if p.id != pertama {
                    peta_nonaktif.insert(p.id.clone(), Instant::now());
                }
            }
            app.manage(AppState {
                konfig: Mutex::new(konfig_awal),
                basis: basis.clone(),
                nonaktif_sejak: Mutex::new(peta_nonaktif),
                tidur: Mutex::new(HashSet::new()),
                tingkat: tingkat0,
                batas: batas0,
                split: Mutex::new(None),
                unduhan_terakhir: Mutex::new(None),
            });
            utas_deepsleep(handle.clone());
            utas_jadwal(handle.clone());
            // Tulis konfig supaya file profiles.json selalu ada & mutakhir.
            if let Some(s) = app.try_state::<AppState>() {
                simpan_konfig(&s);
            }
            log(&format!("konfig ok: {} profil", daftar_awal.len()));

            let window = WindowBuilder::new(app, "main")
                .title("Osvyn")
                .inner_size(1280.0, 800.0)
                .min_inner_size(420.0, 560.0)
                .visible(false)
                .build()?;
            log("jendela utama ok");

            let lebar = 1280.0;
            let tinggi = 800.0;

            // Gerbang lisensi (versi jualan): tanpa lisensi sah yang
            // terbukti di inti Rust, jendela hanya berisi layar aktivasi.
            // Bar, profil, dan splash dibangun hanya pada jalur AKTIF.
            let lisensi_aktif = lisensi0.is_some();
            if !lisensi_aktif {
                let akt = WebviewBuilder::new(
                    "aktivasi",
                    WebviewUrl::App("aktivasi.html".into()),
                );
                window.add_child(
                    akt,
                    LogicalPosition::new(0.0, 0.0),
                    LogicalSize::new(lebar, tinggi),
                )?;
                log("lisensi: TERKUNCI, layar aktivasi tampil");
            } else {
                log("lisensi: AKTIF, jalur penuh");
            log(&format!(
                "gerbang tingkat: {} (batas {}), {} dari {} profil dibangun",
                nama_tingkat(tingkat0),
                if batas0 == usize::MAX {
                    "tanpa batas".to_string()
                } else {
                    batas0.to_string()
                },
                daftar_bangun.len(),
                daftar_awal.len()
            ));
            utas_cek_server(handle.clone(), basis.clone());
            let bar = WebviewBuilder::new("bar", WebviewUrl::App("index.html".into()));
            window.add_child(
                bar,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(lebar, BAR_H),
            )?;
            log("bar profil ok");

            // `pertama` dihitung di awal setup dari konfig.aktif.
            for p in &daftar_bangun {
                let wv = buat_webview(&window, &basis, &p.id, lebar, tinggi)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                if p.id != pertama {
                    let _ = wv.hide();
                }
                log(&format!("webview profil {} ok", p.id));
            }

            // Spark Intro / Shift: splash menutupi seluruh jendela di atas
            // semua webview lain. Halaman splash menutup dirinya lewat
            // perintah tutup_splash (klik / timeout animasi); thread
            // pengaman menyembunyikannya paksa pada 8 detik bila halaman
            // gagal memanggil (JS mati dsb) supaya aplikasi tidak tertahan.
            // (Splash Osvyn kini 6 detik — perintah abang 28 Okt 23:19,
            // rentang yang diminta 5-7 detik; pengaman harus lebih lama
            // dari durasi halamannya.)
            let halaman_splash = if mode_shift {
                "splash_shift.html"
            } else {
                "splash.html"
            };
            let splash = WebviewBuilder::new("splash", WebviewUrl::App(halaman_splash.into()));
            window.add_child(
                splash,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(lebar, tinggi),
            )?;
            log(if mode_shift {
                "splash Shift ok"
            } else {
                "splash Spark Intro ok"
            });
            let handle_pengaman = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(8000));
                let h2 = handle_pengaman.clone();
                let _ = handle_pengaman.run_on_main_thread(move || {
                    sembunyikan_splash(&h2);
                });
            });

            // Hub (v0.1.4): webview layar pusat menutupi seluruh jendela,
            // dibuat tersembunyi; dibuka lewat perintah buka_hub yang
            // digerbang tingkat Hub di inti Rust.
            let hub = WebviewBuilder::new("hub", WebviewUrl::App("hub.html".into()));
            let hubwv = window.add_child(
                hub,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(lebar, tinggi),
            )?;
            let _ = hubwv.hide();
            log("hub ok (tersembunyi)");
            // Sesudah tambah profil dari halaman Hub, aplikasi menyala
            // ulang; penanda hub.flag membuka Hub lagi begitu splash
            // selesai (utas tunda, pola pengaman splash).
            let berkas_hub = basis.join("hub.flag");
            if berkas_hub.exists() && tingkat0 >= 4 {
                let _ = std::fs::remove_file(&berkas_hub);
                let handle_hub = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(9000));
                    let h2 = handle_hub.clone();
                    let _ = handle_hub.run_on_main_thread(move || {
                        if let Some(h) = h2.get_webview("hub") {
                            let _ = h.show();
                            let _ = h.eval("window.__hubMasuk && window.__hubMasuk()");
                        }
                    });
                });
                log("hub: penanda ketemu, Hub dibuka sesudah splash");
            }
            }

            let buka = MenuItem::with_id(app, "buka", "Open Osvyn", true, None::<&str>)?;
            let sembunyi_i =
                MenuItem::with_id(app, "sembunyi", "Hide to Tray", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let keluar_i = MenuItem::with_id(app, "keluar", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&buka, &sembunyi_i, &sep, &keluar_i])?;
            let mut tray = TrayIconBuilder::with_id("osvyn-tray")
                .tooltip("Osvyn")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "buka" => tampil_jendela(app),
                    "sembunyi" => sembunyi_jendela(app),
                    "keluar" => keluar(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        tampil_jendela(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;
            log("tray aktif");

            tampil_jendela(&handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("gagal membangun aplikasi Osvyn");
    log("build ok, event loop mulai");
    app.run(|app, event| match event {
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Resized(size),
            ..
        } if label == "main" => {
            if let Some(w) = app.get_window("main") {
                let skala = w.scale_factor().unwrap_or(1.0);
                let lebar = size.width as f64 / skala;
                let tinggi = size.height as f64 / skala;
                if lebar > 0.0 && tinggi > BAR_H {
                    atur_ukuran(app, lebar, tinggi);
                }
            }
        }
        RunEvent::Exit => {
            log("run event: Exit");
        }
        _ => {}
    });
    log("aplikasi berhenti total");
}
