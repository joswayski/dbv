import AppKit
import SwiftUI

/// A plain `NSTextView` wrapped for SwiftUI.
///
/// SwiftUI's `TextEditor` does not expose the caret or the selection, and DBM's
/// statement targeting needs both, so the SQL editor is AppKit underneath.
struct SQLEditor: NSViewRepresentable {
    @Binding var text: String
    var onSelectionChange: (NSRange) -> Void
    var onRun: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = false

        let textView = DBMTextView()
        textView.delegate = context.coordinator
        textView.isRichText = false
        textView.allowsUndo = true
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.font = Theme.monoNSFont
        textView.textColor = NSColor(Theme.text)
        textView.backgroundColor = NSColor(Theme.editor)
        textView.insertionPointColor = NSColor(Theme.accent)
        textView.textContainerInset = NSSize(width: 8, height: 8)
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.minSize = NSSize(width: 0, height: 0)
        textView.maxSize = NSSize(width: .greatestFiniteMagnitude, height: .greatestFiniteMagnitude)
        textView.textContainer?.containerSize = NSSize(width: 0, height: .greatestFiniteMagnitude)
        textView.textContainer?.widthTracksTextView = true
        textView.string = text
        textView.onCommandReturn = onRun

        scrollView.documentView = textView
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = scrollView.documentView as? DBMTextView else { return }
        context.coordinator.parent = self
        textView.onCommandReturn = onRun
        // Only push text in when it changed elsewhere (a history click), so
        // typing does not fight the binding.
        if textView.string != text {
            let length = (text as NSString).length
            let location = min(textView.selectedRange().location, length)
            textView.string = text
            textView.setSelectedRange(NSRange(location: location, length: 0))
            onSelectionChange(textView.selectedRange())
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: SQLEditor

        init(_ parent: SQLEditor) {
            self.parent = parent
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else { return }
            parent.text = textView.string
            parent.onSelectionChange(textView.selectedRange())
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else { return }
            parent.onSelectionChange(textView.selectedRange())
        }
    }
}

/// Command+Return runs the statement without leaving the editor.
final class DBMTextView: NSTextView {
    var onCommandReturn: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        let isReturn = event.keyCode == 36 || event.keyCode == 76
        if isReturn, event.modifierFlags.contains(.command) {
            onCommandReturn?()
            return
        }
        super.keyDown(with: event)
    }
}
