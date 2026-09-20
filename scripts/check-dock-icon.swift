// Native icon regression: compare the packaged and post-exit system images,
// plus AppKit's restored default captured immediately before quitting.
import AppKit

let arguments = CommandLine.arguments
// The Python check supplies only the PID of its own isolated child process.
if arguments.count == 3 && arguments[1] == "--quit" {
    guard let pid = Int32(arguments[2]), let app = NSRunningApplication(processIdentifier: pid),
        app.bundleIdentifier?.hasPrefix("dev.nus.") == true, app.terminate() else {
        fputs("Could not request native Quit for the test app\n", stderr)
        exit(1)
    }
    exit(0)
}
guard arguments.count >= 4 else {
    fatalError("usage: check-dock-icon.swift bundle.app expected.png quit-default.tiff...")
}
let bundle = URL(fileURLWithPath: arguments[1])
let size = 256

func pixels(_ image: NSImage) -> [UInt8] {
    var rect = CGRect(x: 0, y: 0, width: size, height: size)
    guard let source = image.cgImage(forProposedRect: &rect, context: nil, hints: nil) else {
        fatalError("Icon has no raster representation")
    }
    var bytes = [UInt8](repeating: 0, count: size * size * 4)
    bytes.withUnsafeMutableBytes { storage in
        let context = CGContext(data: storage.baseAddress, width: size, height: size,
            bitsPerComponent: 8, bytesPerRow: size * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.interpolationQuality = .high
        context.draw(source, in: rect)
    }
    return bytes
}

func load(_ path: String) -> NSImage {
    guard let image = NSImage(contentsOfFile: path) else { fatalError("Cannot load \(path)") }
    return image
}
let expected = pixels(load(arguments[2]))
func check(_ label: String, _ image: NSImage) {
    let actual = pixels(image)
    let error = zip(actual, expected).reduce(0.0) { $0 + abs(Double($1.0) - Double($1.1)) }
        / Double(actual.count) / 255.0
    // Different icon resolutions have slightly different antialiased edges.
    // The old protruding n fails by a wide margin (also checked as a fixture).
    if error >= 0.012 {
        fputs("\(label): icon differs from clipped artwork (\(error))\n", stderr)
        exit(1)
    }
    print("PASS \(label): normalized pixel error \(error)")
}
let plist = NSDictionary(contentsOf: bundle.appendingPathComponent("Contents/Info.plist"))!
let resource = plist["CFBundleIconFile"] as! String
check("packaged icon", load(bundle.appendingPathComponent("Contents/Resources/\(resource)").path))
check("system icon after exit", NSWorkspace.shared.icon(forFile: bundle.path))
for path in arguments.dropFirst(3) { check("quit default \(URL(fileURLWithPath: path).lastPathComponent)", load(path)) }
