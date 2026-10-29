fn main() {
    // Manifes ACL aplikasi: WAJIB memuat SEMUA perintah aplikasi.
    // Pelajaran v0.2.3 (regresi): manifes parsial (2 perintah) membuat
    // Tauri menegakkan ACL ke semua perintah; 25 perintah lain yang tidak
    // terdaftar + tidak diizinkan capability menjadi tidak bisa dipanggil
    // dari webview mana pun (bar kosong, aplikasi stuck).
    let atribut = tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "aktivasi_kunci", "ambil_bahasa", "simpan_bahasa",
            "daftar_profil", "ganti_profil", "tambah_profil",
            "simpan_unduhan_blob", "catat_jembatan_unduhan", "ganti_nama",
            "hapus_profil", "profil_aktif", "profil_tidur",
            "tutup_splash", "info_paket", "data_hub",
            "buka_hub", "tutup_hub", "tandai_hub",
            "tidurkan_profil", "tidurkan_semua", "data_aktivitas",
            "simpan_meta_profil", "simpan_jadwal", "data_jadwal",
            "buka_grup", "buka_split", "tutup_split", "buka_lokasi_unduhan"
        ]),
    );
    tauri_build::try_build(atribut).expect("gagal menjalankan tauri-build");
}
