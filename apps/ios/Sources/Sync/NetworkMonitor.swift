import Foundation
import Network

protocol NetworkMonitorProtocol: AnyObject {
    var isOnline: Bool { get }
    var onStatusChange: ((Bool) -> Void)? { get set }
    func start()
    func stop()
}

final class PathNetworkMonitor: NetworkMonitorProtocol {
    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "app.reinforce.ios.network-monitor", qos: .utility)
    private let lock = NSLock()
    private var _isOnline: Bool = true
    private var started = false

    var onStatusChange: ((Bool) -> Void)?

    var isOnline: Bool {
        lock.lock()
        defer { lock.unlock() }
        return _isOnline
    }

    func start() {
        lock.lock()
        if started {
            lock.unlock()
            return
        }
        started = true
        lock.unlock()

        monitor.pathUpdateHandler = { [weak self] path in
            guard let self else { return }
            let online = path.status == .satisfied
            self.lock.lock()
            let changed = self._isOnline != online
            self._isOnline = online
            self.lock.unlock()

            if changed {
                DispatchQueue.main.async { [weak self] in
                    self?.onStatusChange?(online)
                }
            }
        }
        monitor.start(queue: queue)
    }

    func stop() {
        lock.lock()
        let wasStarted = started
        started = false
        lock.unlock()
        if wasStarted {
            monitor.cancel()
        }
    }
}
