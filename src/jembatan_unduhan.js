// Jembatan unduhan blob untuk Osvyn (diinjeksikan Rust ke webview profil).
//
// Bukti uji native 29 Okt 17:00 (v0.2.1): unduhan lampiran muse.ai adalah
// URL blob: — handler unduhan native WebView2 menerima event Requested
// (tujuan berkas diterima) tapi unduhannya tidak pernah selesai: nol
// event Finished, nol berkas di folder Downloads. Jembatan ini memotong
// jalur itu khusus untuk klik unduh blob: isi blob dibaca di halaman,
// dikirim ke perintah Rust `simpan_unduhan_blob` yang menulisnya ke
// folder Downloads Windows + memberi kabar ke bar profil.
//
// Perbaikan v0.2.3 (bukti log PC 29 Okt 17:30 + sumber Tauri 2.12.5):
// 1. Izin: invoke dari origin remote ditolak Tauri ("Command ... not
//    allowed by ACL") bila perintah aplikasi tidak didaftarkan di
//    build.rs (AppManifest) dan tidak diberi allow-* di capability.
//    Sisi Rust/capability diperbaiki di ronde ini; jembatan juga kini
//    melapor hidup lewat perintah `catat_jembatan_unduhan` sekali per
//    muatan halaman, supaya log aplikasi membuktikan jalur ini aktif.
// 2. Cakupan klik: v0.2.2 hanya mencegat <a download href="blob:"> dari
//    klik betulan. Pola unduh lampiran umumnya programatis (halaman
//    membuat <a> sementara lalu memanggil .click()) — kini setiap klik
//    programatis pada jangkar blob: dicegat, apa pun atributnya. Klik
//    betulan pengguna pada tautan blob: TANPA atribut download (mis.
//    pratinjau gambar) tetap tidak disentuh.
// Berkas > 40 MB tidak dicegat (dilepas ke jalur native) supaya IPC
// tidak menelan memori berlebihan.
(function () {
  if (window.__jembatanUnduhan) return;
  window.__jembatanUnduhan = true;
  var BATAS = 40 * 1024 * 1024;

  function panggilRust(perintah, args) {
    try {
      return window.__TAURI__.core.invoke(perintah, args);
    } catch (e) {
      return Promise.reject(e);
    }
  }

  // Penanda hidup: satu laporan per muatan halaman.
  panggilRust('catat_jembatan_unduhan', { url: String(location.href) }).catch(
    function () {}
  );

  function kirimKeRust(nama, blob) {
    var pembaca = new FileReader();
    pembaca.onload = function () {
      var hasil = String(pembaca.result || '');
      var koma = hasil.indexOf(',');
      var b64 = koma >= 0 ? hasil.slice(koma + 1) : '';
      panggilRust('simpan_unduhan_blob', { nama: nama, isiB64: b64 }).catch(
        function (e) {
          console.warn('jembatan unduhan gagal:', e);
        }
      );
    };
    pembaca.readAsDataURL(blob);
  }

  // Animasi masuk saat profil ini ditampilkan (dipanggil Rust dari
  // aktifkan() tiap pindah profil): halaman bergeser masuk halus
  // sekali, lalu kelas dibersihkan supaya tidak mengganggu halaman.
  window.__pindahMasuk = function () {
    var el = document.documentElement;
    if (!document.getElementById('osvyn-gaya-masuk')) {
      var st = document.createElement('style');
      st.id = 'osvyn-gaya-masuk';
      st.textContent =
        '@keyframes osvynMasuk{from{opacity:0;transform:translateY(8px)}to{opacity:1;transform:translateY(0)}}' +
        'html.osvyn-masuk{animation:osvynMasuk .26s cubic-bezier(.21,.9,.28,1)}';
      (document.head || el).appendChild(st);
    }
    el.classList.remove('osvyn-masuk');
    void el.offsetWidth;
    el.classList.add('osvyn-masuk');
    setTimeout(function () {
      el.classList.remove('osvyn-masuk');
    }, 340);
  };

  var klikAsli = HTMLAnchorElement.prototype.click;

  function tangani(a, programatis) {
    var href = a.getAttribute('href') || '';
    if (href.indexOf('blob:') !== 0) return false;
    var mauUnduh =
      programatis ||
      a.hasAttribute('download') ||
      (a.download || '') !== '' ||
      !document.contains(a);
    if (!mauUnduh) return false;
    var nama = a.getAttribute('download') || a.download || '';
    fetch(href)
      .then(function (r) {
        return r.blob();
      })
      .then(function (blob) {
        if (blob.size > BATAS) {
          // Terlalu besar untuk jembatan: lepaskan ke jalur native
          // dengan klik asli (bukan klik yang sudah dicegat).
          klikAsli.call(a);
          return;
        }
        if (!nama) {
          var petaEkst = {
            'text/plain': 'txt',
            'application/pdf': 'pdf',
            'image/png': 'png',
            'image/jpeg': 'jpg',
            'image/webp': 'webp',
            'application/zip': 'zip',
            'application/json': 'json',
            'text/markdown': 'md',
            'text/csv': 'csv',
          };
          var ekst =
            petaEkst[blob.type] ||
            (blob.type.split('/')[1] || '').split('+')[0].replace(/[^a-z0-9]/gi, '');
          nama = 'unduhan' + (ekst ? '.' + ekst : '');
        }
        kirimKeRust(nama, blob);
      })
      .catch(function () {
        // Blob sudah tidak bisa dibaca (mis. sudah dicabut halaman):
        // tidak ada yang bisa diselamatkan dari sisi jembatan.
      });
    return true;
  }

  // Jalur 1: klik betulan dari pengguna pada tautan unduh yang terlihat.
  document.addEventListener(
    'click',
    function (ev) {
      var a = ev.target;
      while (a && a.tagName !== 'A') a = a.parentElement;
      if (!a) return;
      if (tangani(a, false)) {
        // Cegat supaya jalur native yang mati itu tidak ikut jalan.
        ev.preventDefault();
        ev.stopImmediatePropagation();
      }
    },
    true
  );

  // Jalur 2: klik programatis halaman (pola umum unduhan lampiran:
  // halaman membuat <a> sementara lalu memanggil .click()). Jangkar
  // yang tidak menempel di DOM tidak mengantar event-nya ke document,
  // jadi method click-nya yang dicegat langsung.
  HTMLAnchorElement.prototype.click = function () {
    if (tangani(this, true)) return;
    return klikAsli.apply(this, arguments);
  };
})();
