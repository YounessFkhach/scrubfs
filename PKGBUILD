# Maintainer: Youness Fkhach <fkhachyouness@gmail.com>
pkgname=scrubfs
pkgver=0.1.0
pkgrel=1
pkgdesc='Virtual filesystem that strips file metadata on read'
arch=('x86_64' 'aarch64')
url='https://github.com/YounessFkhach/scrubfs'
license=('MIT')
depends=('fuse3' 'mat2')
makedepends=('cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
    cd "$pkgname-$pkgver"
    cargo fetch --locked --target "$CARCH-unknown-linux-gnu"
}

build() {
    cd "$pkgname-$pkgver"
    cargo build --frozen --release
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm755 "target/release/$pkgname" -t "$pkgdir/usr/bin/"
    install -Dm644 LICENSE -t "$pkgdir/usr/share/licenses/$pkgname/"
    for f in man/*.1; do
        install -Dm644 "$f" -t "$pkgdir/usr/share/man/man1/"
    done
}
