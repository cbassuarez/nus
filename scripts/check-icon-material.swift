// Compare actual AppKit icon readbacks and source PNGs in the same sRGB space.
import AppKit
import Foundation

func pixels(_ path: String) -> [UInt8] {
    guard let image = NSImage(contentsOfFile: path) else { fatalError("Cannot load \(path)") }
    var rect = CGRect(x: 0, y: 0, width: 256, height: 256)
    guard let source = image.cgImage(forProposedRect: &rect, context: nil, hints: nil) else {
        fatalError("No raster representation: \(path)")
    }
    precondition(source.width == 256 && source.height == 256, "Unexpected icon size: \(path)")
    var bytes = [UInt8](repeating: 0, count: 256 * 256 * 4)
    bytes.withUnsafeMutableBytes { storage in
        let context = CGContext(data: storage.baseAddress, width: 256, height: 256,
            bitsPerComponent: 8, bytesPerRow: 256 * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.draw(source, in: rect)
    }
    return bytes
}

let paths = Array(CommandLine.arguments.dropFirst())
precondition(paths.count > 0 && paths.count % 2 == 0, "Supply actual.tiff expected.png pairs")
var results = [[String: Any]]()
for at in stride(from: 0, to: paths.count, by: 2) {
    let actual = pixels(paths[at]), expected = pixels(paths[at + 1])
    var errors = [Int]()
    var alphaError = 0
    for i in stride(from: 0, to: actual.count, by: 4) {
        alphaError = max(alphaError, abs(Int(actual[i + 3]) - Int(expected[i + 3])))
        if actual[i + 3] > 250 && expected[i + 3] > 250 {
            for channel in 0..<3 { errors.append(abs(Int(actual[i + channel]) - Int(expected[i + channel]))) }
        }
    }
    precondition(!errors.isEmpty, "No opaque pixels")
    errors.sort()
    let mean = Double(errors.reduce(0, +)) / Double(errors.count)
    let p95 = errors[Int(Double(errors.count - 1) * 0.95)]
    results.append(["native_tiff": paths[at], "opaque_rgb_mean_error": mean,
        "opaque_rgb_p95_error": p95, "alpha_max_error": alphaError])
}
let data = try JSONSerialization.data(withJSONObject: results, options: [.sortedKeys])
print(String(data: data, encoding: .utf8)!)
