fn main() {
    // Sem CRT: a DLL nao tem ponto de entrada nem bibliotecas padrao.
    println!("cargo:rustc-cdylib-link-arg=/NOENTRY");
    println!("cargo:rustc-cdylib-link-arg=/NODEFAULTLIB");
}
