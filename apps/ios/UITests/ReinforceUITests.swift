import XCTest

final class ReinforceUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    private var isPad: Bool {
        UIDevice.current.userInterfaceIdiom == .pad
    }

    private func fixturePath(_ name: String) throws -> String {
        let bundle = Bundle(for: Self.self)
        guard let url = bundle.url(forResource: name, withExtension: nil) else {
            throw XCTSkip("Fixture \(name) missing from UI test bundle")
        }
        return url.path
    }

    private func attachScreenshot(_ name: String) {
        let deviceTag = isPad ? "ipad" : "iphone"
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "\(deviceTag)-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func ensureSidebarVisibleIfPad(_ app: XCUIApplication) {
        guard isPad else { return }
        if !app.buttons["sidebar-study"].exists && !app.staticTexts["刷題室"].exists {
            let toggle = app.navigationBars.buttons.element(boundBy: 0)
            if toggle.exists {
                toggle.tap()
            }
        }
    }

    private func openStudyTab(_ app: XCUIApplication) {
        if isPad {
            ensureSidebarVisibleIfPad(app)
            let sidebarItem = app.descendants(matching: .any)["sidebar-study"].firstMatch
            if sidebarItem.waitForExistence(timeout: 5) {
                sidebarItem.tap()
                return
            }
            let fallback = app.staticTexts["刷題室"].firstMatch
            XCTAssertTrue(fallback.waitForExistence(timeout: 5))
            fallback.tap()
        } else {
            let tab = app.tabBars.buttons["刷題室"]
            XCTAssertTrue(tab.waitForExistence(timeout: 5))
            tab.tap()
        }
    }

    private func openPracticalTab(_ app: XCUIApplication) {
        if isPad {
            ensureSidebarVisibleIfPad(app)
            let sidebarItem = app.descendants(matching: .any)["sidebar-practical"].firstMatch
            if sidebarItem.waitForExistence(timeout: 5) {
                sidebarItem.tap()
                return
            }
            let fallback = app.staticTexts["實務跑考"].firstMatch
            XCTAssertTrue(fallback.waitForExistence(timeout: 5))
            fallback.tap()
        } else {
            let tab = app.tabBars.buttons["實務跑考"]
            XCTAssertTrue(tab.waitForExistence(timeout: 5))
            tab.tap()
        }
    }

    private func waitForImageLoaded(_ app: XCUIApplication, id: String, timeout: TimeInterval = 12) {
        let element = app.descendants(matching: .any)[id].firstMatch
        XCTAssertTrue(element.waitForExistence(timeout: timeout), "Missing image element \(id)")
        let loadedPredicate = NSPredicate(format: "value == %@", "loaded")
        let loadedExpectation = expectation(for: loadedPredicate, evaluatedWith: element)
        wait(for: [loadedExpectation], timeout: timeout)
    }

    private func tapButton(_ app: XCUIApplication, id: String, timeout: TimeInterval = 6) {
        let btn = app.buttons[id].firstMatch
        XCTAssertTrue(btn.waitForExistence(timeout: timeout), "Missing button \(id)")
        var attempts = 0
        while !btn.isHittable && attempts < 4 {
            app.swipeUp()
            attempts += 1
        }
        btn.tap()
    }

    func testDemoImportReadQuizAndPersistence() throws {
        let demoPath = try fixturePath("demo")
        let app = XCUIApplication()
        app.launchArguments = [
            "-uitest-reset",
            "-uitest-language", "zh-TW",
            "-uitest-import-path", demoPath
        ]
        app.launch()

        if isPad {
            XCTAssertTrue(app.descendants(matching: .any)["ipad-sidebar"].firstMatch.waitForExistence(timeout: 6))
        }

        // 1. Import fixtures/demo
        tapButton(app, id: "empty-import-button", timeout: 8)
        XCTAssertTrue(app.descendants(matching: .any)["import-success-banner"].firstMatch.waitForExistence(timeout: 8))

        // 2. Open the note "相位變化入門" in Reading Room
        tapButton(app, id: "note-row-相位變化入門", timeout: 8)

        // Verify transcluded cards, interactive pins base diagram loaded, & overlay toggle
        XCTAssertTrue(app.staticTexts["水的三態與相變"].waitForExistence(timeout: 8))
        let overlayToggle = app.buttons["pins-overlay-toggle"].firstMatch
        if !isPad {
            // Scroll gently so both the GFM table and interactive pins diagram are clearly visible
            let start = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.72))
            let end = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.36))
            start.press(forDuration: 0.05, thenDragTo: end)
        }
        waitForImageLoaded(app, id: "interactive-pins-base-image", timeout: 12)
        if !overlayToggle.isHittable {
            app.swipeUp()
        }
        XCTAssertTrue(overlayToggle.waitForExistence(timeout: 5), "Missing pins-overlay-toggle")
        overlayToggle.tap()
        waitForImageLoaded(app, id: "interactive-pins-overlay-image", timeout: 12)
        attachScreenshot("reading-room")

        // 3. Navigate to Study Hub and verify all 5 modes are available
        openStudyTab(app)
        XCTAssertTrue(app.buttons["study-mode-daily_due"].waitForExistence(timeout: 6))
        XCTAssertTrue(app.buttons["study-mode-full_exam"].exists)
        XCTAssertTrue(app.buttons["study-mode-topic_drill"].exists)
        XCTAssertTrue(app.buttons["study-mode-practical_exam"].exists)
        XCTAssertTrue(app.buttons["study-mode-weakness_drill"].exists)

        // Start Daily Due quiz and answer all 4 demo questions regardless of FSRS/UUID ordering
        tapButton(app, id: "study-mode-daily_due")
        tapButton(app, id: "study-start-session-button")
        XCTAssertTrue(app.descendants(matching: .any)["quiz-progress-label"].firstMatch.waitForExistence(timeout: 8))

        var capturedQuizShot = false
        for _ in 0..<4 {
            if app.buttons["quiz-option-D"].firstMatch.waitForExistence(timeout: 2) {
                // Q2: Multiple choice (A = 熔化, C = 汽化)
                tapButton(app, id: "quiz-option-A")
                tapButton(app, id: "quiz-option-C")
            } else if app.buttons["quiz-option-A"].firstMatch.exists {
                // Q1: Single choice (A = 吸收熱量)
                tapButton(app, id: "quiz-option-A")
                if !capturedQuizShot {
                    attachScreenshot("quiz-runner")
                    capturedQuizShot = true
                }
            } else {
                // Q3 (short_answer) or Q4 (voided placeholder)
                let submitBtn = app.buttons["quiz-submit-button"].firstMatch
                if !submitBtn.isEnabled {
                    let textField = app.textFields["quiz-text-answer-field"].firstMatch
                    XCTAssertTrue(textField.waitForExistence(timeout: 4))
                    textField.tap()
                    textField.typeText("boiling\n")
                }
            }

            if app.buttons["quiz-submit-button"].firstMatch.exists && app.buttons["quiz-submit-button"].firstMatch.isEnabled {
                tapButton(app, id: "quiz-submit-button")
            }
            XCTAssertTrue(app.descendants(matching: .any)["quiz-feedback-banner"].firstMatch.waitForExistence(timeout: 6))
            tapButton(app, id: "quiz-next-button")
        }

        // Verify score summary
        let scoreLabel = app.staticTexts["quiz-summary-score"].firstMatch
        XCTAssertTrue(scoreLabel.waitForExistence(timeout: 8))
        XCTAssertEqual(scoreLabel.label, "100%")
        XCTAssertTrue(app.staticTexts["quiz-summary-attempts-saved"].firstMatch.exists)
        attachScreenshot("quiz-summary")

        tapButton(app, id: "quiz-summary-done-button")

        // 4. Relaunch App WITHOUT -uitest-reset and verify records persisted
        app.terminate()
        let relaunched = XCUIApplication()
        relaunched.launchArguments = ["-uitest-language", "zh-TW"]
        relaunched.launch()

        XCTAssertTrue(relaunched.buttons["note-row-相位變化入門"].firstMatch.waitForExistence(timeout: 8))
        openStudyTab(relaunched)
        let reviewedStat = relaunched.staticTexts["study-stat-reviewed"].firstMatch
        XCTAssertTrue(reviewedStat.waitForExistence(timeout: 6))
        XCTAssertNotEqual(reviewedStat.label, "0", "Persisted FSRS review count must remain non-zero after relaunch")
    }

    func testPracticalExamPinZoomAndBackgroundPause() throws {
        let practicalPath = try fixturePath("practical_demo")
        let app = XCUIApplication()
        app.launchArguments = [
            "-uitest-reset",
            "-uitest-language", "zh-TW",
            "-uitest-import-path", practicalPath
        ]
        app.launch()

        // Import practical fixture
        tapButton(app, id: "empty-import-button", timeout: 8)
        XCTAssertTrue(app.descendants(matching: .any)["import-success-banner"].firstMatch.waitForExistence(timeout: 8))

        // Open Practical Exam tab & start session
        openPracticalTab(app)
        tapButton(app, id: "study-start-session-button")

        // Verify timer & zoomable canvas with loaded base diagram
        XCTAssertTrue(app.descendants(matching: .any)["quiz-timer-badge"].firstMatch.waitForExistence(timeout: 8))
        let canvas = app.otherElements["practical-canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 6))
        waitForImageLoaded(app, id: "practical-canvas-base-image", timeout: 12)

        // Exercise zoom in (toolbar + pinch-to-zoom)
        tapButton(app, id: "practical-zoom-in")
        XCTAssertEqual(app.staticTexts["practical-zoom-percentage"].firstMatch.label, "150%")
        attachScreenshot("practical-exam")
        canvas.pinch(withScale: 1.4, velocity: 1.0)
        tapButton(app, id: "practical-zoom-reset")

        // Send app to background and verify countdown pauses automatically
        XCUIDevice.shared.press(.home)
        sleep(2)
        app.activate()

        let bgNotice = app.descendants(matching: .any)["quiz-paused-background-notice"].firstMatch
        XCTAssertTrue(bgNotice.waitForExistence(timeout: 8), "Expected automatic pause notice after returning from background")
        tapButton(app, id: "quiz-resume-overlay-button")

        // Answer both practical questions (pin_identify & pin_locate) in either order
        for _ in 0..<2 {
            if app.textFields["quiz-text-answer-field"].firstMatch.waitForExistence(timeout: 3) {
                let textField = app.textFields["quiz-text-answer-field"].firstMatch
                textField.tap()
                textField.typeText("triple point\n")
            } else {
                XCTAssertTrue(canvas.waitForExistence(timeout: 4))
                canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.72, dy: 0.30)).tap()
            }
            if app.buttons["quiz-submit-button"].firstMatch.exists && app.buttons["quiz-submit-button"].firstMatch.isEnabled {
                tapButton(app, id: "quiz-submit-button")
            }
            XCTAssertTrue(app.descendants(matching: .any)["quiz-feedback-banner"].firstMatch.waitForExistence(timeout: 6))
            tapButton(app, id: "quiz-next-button")
        }

        XCTAssertTrue(app.staticTexts["quiz-summary-score"].firstMatch.waitForExistence(timeout: 8))
    }

    func testSyncSheetExportPresetsAndDeepLinkCard() throws {
        let demoPath = try fixturePath("demo")
        let app = XCUIApplication()
        app.launchArguments = [
            "-uitest-reset",
            "-uitest-language", "zh-TW",
            "-uitest-import-path", demoPath
        ]
        app.launch()

        // 1. Import demo content
        tapButton(app, id: "empty-import-button", timeout: 8)
        XCTAssertTrue(app.descendants(matching: .any)["import-success-banner"].firstMatch.waitForExistence(timeout: 8))

        // 2. Open Server & Sync sheet and verify inputs
        if isPad {
            ensureSidebarVisibleIfPad(app)
            let sidebarSync = app.buttons["sidebar-sync-button"].firstMatch
            if sidebarSync.waitForExistence(timeout: 5) {
                sidebarSync.tap()
            } else {
                tapButton(app, id: "toolbar-sync-button", timeout: 6)
            }
        } else {
            tapButton(app, id: "toolbar-sync-button", timeout: 6)
        }
        XCTAssertTrue(app.textFields["sync-server-url-field"].firstMatch.waitForExistence(timeout: 6))
        XCTAssertTrue(app.buttons["sync-sign-in-button"].firstMatch.exists)
        attachScreenshot("server-sync-sheet")
        tapButton(app, id: "sync-sheet-done-button")

        // 3. Open note and verify PDF Export sheet shows all three presets
        tapButton(app, id: "note-row-相位變化入門", timeout: 8)
        tapButton(app, id: "note-export-pdf-button", timeout: 6)
        XCTAssertTrue(app.buttons["export-preset-standard"].firstMatch.waitForExistence(timeout: 6))
        XCTAssertTrue(app.buttons["export-preset-goodnotes"].firstMatch.exists)
        XCTAssertTrue(app.buttons["export-preset-worksheet"].firstMatch.exists)
        attachScreenshot("pdf-export-sheet")
        tapButton(app, id: "export-sheet-close-button")

        // 4. Open reinforce://card/<id> deep link (without -uitest-reset so imported data stays intact)
        app.launchArguments = ["-uitest-language", "zh-TW"]
        let deepLinkURL = try XCTUnwrap(URL(string: "reinforce://card/01a21714-c98d-716e-9b53-ead7c8ee810f"))
        app.open(deepLinkURL)

        XCTAssertTrue(app.staticTexts["水的三態與相變"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.descendants(matching: .any)["card-detail-content"].firstMatch.waitForExistence(timeout: 6))
        attachScreenshot("deeplink-card")
    }
}
