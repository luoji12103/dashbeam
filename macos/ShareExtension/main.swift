import AppKit
import UniformTypeIdentifiers

@_silgen_name("NSExtensionMain")
func extensionMain(_ argc: Int32, _ argv: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Int32

@objc(DashBeamShareViewController)
final class ShareViewController: NSViewController {
    private let label = NSTextField(wrappingLabelWithString: "正在将文件添加到 DashBeam…")
    private var started = false

    override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 360, height: 130))
        label.frame = NSRect(x: 24, y: 55, width: 312, height: 50)
        view.addSubview(label)
        let cancel = NSButton(title: "关闭", target: self, action: #selector(close))
        cancel.frame = NSRect(x: 262, y: 15, width: 74, height: 28)
        view.addSubview(cancel)
        preferredContentSize = view.frame.size
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        guard !started else { return }
        started = true
        let providers = (extensionContext?.inputItems as? [NSExtensionItem] ?? [])
            .flatMap { $0.attachments ?? [] }
        guard !providers.isEmpty else { label.stringValue = "没有可分享的文件。"; return }
        let group = DispatchGroup()
        let lock = NSLock()
        var files = [Int: URL]()
        for (index, provider) in providers.enumerated() {
            guard provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) else { continue }
            group.enter()
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, _ in
                let url: URL?
                if let value = item as? URL { url = value }
                else if let data = item as? Data { url = URL(dataRepresentation: data, relativeTo: nil) }
                else if let value = item as? String { url = URL(string: value) }
                else { url = nil }
                if let url, url.isFileURL {
                    lock.lock(); files[index] = url; lock.unlock()
                }
                group.leave()
            }
        }
        group.notify(queue: .main) { [weak self] in
            guard let self else { return }
            guard files.count == providers.count else {
                self.label.stringValue = "部分项目无法读取。请在 Finder 中选取本地文件后重试。"
                return
            }
            var components = URLComponents()
            components.scheme = "dashbeam-local"
            components.host = "share"
            components.queryItems = files.keys.sorted().compactMap { index in
                files[index].map { URLQueryItem(name: "file", value: $0.absoluteString) }
            }
            guard let url = components.url else { return }
            // Pin the receiver to our containing app instead of trusting the
            // system's default handler for a custom URL scheme.
            let host = Bundle.main.bundleURL.deletingLastPathComponent()
                .deletingLastPathComponent().deletingLastPathComponent()
            let configuration = NSWorkspace.OpenConfiguration()
            configuration.activates = true
            NSWorkspace.shared.open([url], withApplicationAt: host, configuration: configuration) { _, error in
                DispatchQueue.main.async {
                    if error != nil {
                        self.label.stringValue = "无法打开 DashBeam，请先手动启动客户端后重试。"
                    } else {
                        self.extensionContext?.completeRequest(returningItems: nil)
                    }
                }
            }
        }
    }

    @objc private func close() {
        extensionContext?.cancelRequest(withError: NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError))
    }
}

exit(extensionMain(CommandLine.argc, CommandLine.unsafeArgv))
