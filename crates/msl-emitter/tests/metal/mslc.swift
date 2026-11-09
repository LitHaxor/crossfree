// Compiles Metal Shading Language files with the system Metal compiler, the
// way the runtime will (MTLDevice.makeLibrary(source:)), and checks each has
// the `shader_main` entry point. Used by tests/metal_compile.rs; needs no
// Xcode, only swiftc (Command Line Tools) and a Metal device.
//
// Usage: mslc file.metal...   Prints "OK <file>" or "FAIL <file>" plus the
// compiler log for each; exits 1 if any failed.

import Foundation
import Metal

guard let device = MTLCreateSystemDefaultDevice() else {
    print("no Metal device")
    exit(2)
}

var failed = 0
for path in CommandLine.arguments.dropFirst() {
    guard let source = try? String(contentsOfFile: path, encoding: .utf8) else {
        print("FAIL \(path)\nunreadable")
        failed += 1
        continue
    }
    do {
        let library = try device.makeLibrary(source: source, options: MTLCompileOptions())
        if library.makeFunction(name: "shader_main") == nil {
            print("FAIL \(path)\nno shader_main function")
            failed += 1
        } else {
            print("OK \(path)")
        }
    } catch {
        print("FAIL \(path)\n\(error.localizedDescription)")
        failed += 1
    }
}
exit(failed == 0 ? 0 : 1)
