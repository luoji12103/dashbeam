#import <AppKit/AppKit.h>

// Compile the production implementation into this standalone process so the
// proof exercises exactly the same global monitor, hot zone, and NSPanel.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wnullability-completeness"
#import "../src-tauri/native/flash_drop.m"
#pragma clang diagnostic pop

static void IgnoreDroppedFiles(const char *const *paths, size_t count) {
    (void)paths;
    (void)count;
}

int main(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];

        dashbeam_flash_drop_configure("离线测试设备", false, IgnoreDroppedFiles);

        __block NSUInteger eventCount = 0;
        __block BOOL panelShown = NO;
        __block id proofMonitor = [NSEvent
            addGlobalMonitorForEventsMatchingMask:NSEventMaskLeftMouseDragged
                                          handler:^(NSEvent *event) {
                                            (void)event;
                                            eventCount += 1;
                                            // The production monitor was installed first. Checking on
                                            // the next main-queue turn also avoids depending on monitor
                                            // callback ordering.
                                            dispatch_async(dispatch_get_main_queue(), ^{
                                              if (gController.panel.isVisible) {
                                                  panelShown = YES;
                                              }
                                            });
                                          }];

        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(30 * NSEC_PER_SEC)),
                       dispatch_get_main_queue(), ^{
                         if (proofMonitor != nil) {
                             [NSEvent removeMonitor:proofMonitor];
                             proofMonitor = nil;
                         }

                         // Clean up synchronously before ending the process. The public
                         // C stop function dispatches asynchronously because production
                         // callers may be off-main-thread.
                         [gController stop];
                         gController = nil;
                         gFilesCallback = NULL;

                         fprintf(stdout,
                                 "panel-shown=%s event-count=%lu\n",
                                 panelShown ? "true" : "false",
                                 (unsigned long)eventCount);
                         fflush(stdout);
                         [NSApp terminate:nil];
                       });

        [NSApp run];
    }
    return 0;
}
