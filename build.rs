//! Build script to compile userspace programs and embed them in kernel

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let target_dir = Path::new(&out_dir).join("userspace");
    
    // Create target directory
    fs::create_dir_all(&target_dir).unwrap();
    
    // Compile userspace programs
    compile_userspace_program("vga_driver", &target_dir);
    compile_userspace_program("init", &target_dir);
    compile_userspace_program("shell", &target_dir);
    
    // Generate embedded programs file
    generate_embedded_programs(&target_dir);
    
    println!("cargo:rerun-if-changed=userspace/");
}

fn compile_userspace_program(name: &str, target_dir: &Path) {
    let manifest_path = format!("userspace/{}/Cargo.toml", name);
    let target_file = target_dir.join(format!("{}.bin", name));
    
    println!("cargo:rerun-if-changed={}", manifest_path);
    
    // Compile the userspace program
    let output = Command::new("cargo")
        .args(&[
            "+nightly",
            "-Zbuild-std=core,compiler_builtins",
            "-Zbuild-std-features=compiler-builtins-mem",
            "-Zjson-target-spec",
            "build",
            "--release",
            "--manifest-path", &manifest_path,
            "--target", "x86_64-minux.json", // Use our custom target
        ])
        .output()
        .expect(&format!("Failed to compile {}", name));
    
    if !output.status.success() {
        panic!("Failed to compile {}: {}", name, String::from_utf8_lossy(&output.stderr));
    }
    
    // Copy binary to target directory
    let binary_path = format!("target/x86_64-minux/release/{}", name);
    if Path::new(&binary_path).exists() {
        fs::copy(&binary_path, &target_file).unwrap();
        println!("Compiled {} -> {}", name, target_file.display());
    }
}

fn generate_embedded_programs(target_dir: &Path) {
    let mut code = String::new();
    code.push_str("//! Embedded userspace programs\n\n");
    
    for program in &["vga_driver", "init", "shell"] {
        let bin_file = target_dir.join(format!("{}.bin", program));
        
        if bin_file.exists() {
            let data = fs::read(&bin_file).unwrap();
            
            code.push_str(&format!(
                "pub static {}_PROGRAM: &[u8] = &[\n",
                program.to_uppercase()
            ));
            
            // Write bytes in chunks of 16
            for chunk in data.chunks(16) {
                code.push_str("    ");
                for (i, byte) in chunk.iter().enumerate() {
                    if i > 0 {
                        code.push_str(", ");
                    }
                    code.push_str(&format!("0x{:02x}", byte));
                }
                code.push_str(",\n");
            }
            
            code.push_str("];\n\n");
        } else {
            // Create empty program if compilation failed
            code.push_str(&format!(
                "pub static {}_PROGRAM: &[u8] = &[];\n\n",
                program.to_uppercase()
            ));
        }
    }
    
    // Write embedded programs file
    let embedded_file = Path::new(&env::var("OUT_DIR").unwrap()).join("embedded_programs.rs");
    fs::write(embedded_file, code).unwrap();
}
