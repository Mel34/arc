# Maintainer: Petar Margetić

pkgname=arc
pkgver=0.2.0
pkgrel=1
pkgdesc='TUI AUR package manager with Local repository'
arch=('x86_64')
url=''
license=('GPL-3.0-or-later')
depends=()
optdepends=()
makedepends=('cargo')

build() {
    cd "$HOME/Projects/$pkgname"

    LTOFLAGS= CFLAGS="${CFLAGS//-flto=auto/}" LDFLAGS="${LDFLAGS//-flto=auto/}" \
        cargo build --release --locked
}

check() {
    cd "$HOME/Projects/$pkgname"

    cargo test --release --locked
}

package() {
    cd "$HOME/Projects/$pkgname"

     install -Dm755 "target/release/$pkgname" \
         "$pkgdir/usr/bin/$pkgname"
}
