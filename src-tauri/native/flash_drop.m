#import <AppKit/AppKit.h>
#include <stdbool.h>

typedef void (*DBFlashDropFilesCallback)(const char *const *paths, size_t count);

static DBFlashDropFilesCallback gFilesCallback = NULL;

@class DBFlashDropController;

@interface DBFlashDropView : NSView <NSDraggingDestination>
@property(nonatomic, weak) DBFlashDropController *controller;
@property(nonatomic, copy) NSString *titleText;
@property(nonatomic, copy) NSString *statusText;
@property(nonatomic) BOOL targetAvailable;
@property(nonatomic) BOOL dragHovering;
@end

@interface DBFlashDropPanel : NSPanel
@end

@interface DBFlashDropController : NSObject
@property(nonatomic, strong) DBFlashDropPanel *panel;
@property(nonatomic, strong) DBFlashDropView *dropView;
@property(nonatomic, strong) id dragMonitor;
@property(nonatomic, strong) id mouseUpMonitor;
@property(nonatomic, strong) id localMonitor;
@property(nonatomic, copy) NSString *targetLabel;
@property(nonatomic) BOOL targetAvailable;
@property(nonatomic) BOOL retainingResult;
@property(nonatomic) NSUInteger resultGeneration;

- (void)configureWithLabel:(NSString *)label available:(BOOL)available;
- (void)stop;
- (void)handleDragEvent:(NSEvent *)event;
- (void)handleMouseUp;
- (void)handleDropURLs:(NSArray<NSURL *> *)urls;
- (void)showResult:(BOOL)success message:(NSString *)message;
@end

static DBFlashDropController *gController = nil;

static NSArray<NSURL *> *DBFileURLsFromPasteboard(NSPasteboard *pasteboard) {
    if (pasteboard == nil) {
        return @[];
    }

    NSDictionary *options = @{NSPasteboardURLReadingFileURLsOnlyKey : @YES};
    NSArray *objects = [pasteboard readObjectsForClasses:@[[NSURL class]] options:options];
    NSMutableArray<NSURL *> *fileURLs = [NSMutableArray array];
    for (id object in objects) {
        if ([object isKindOfClass:[NSURL class]] && [(NSURL *)object isFileURL]) {
            [fileURLs addObject:object];
        }
    }
    return fileURLs;
}

@implementation DBFlashDropPanel

- (BOOL)canBecomeKeyWindow {
    return NO;
}

- (BOOL)canBecomeMainWindow {
    return NO;
}

@end

@implementation DBFlashDropView

- (instancetype)initWithFrame:(NSRect)frameRect {
    self = [super initWithFrame:frameRect];
    if (self) {
        _titleText = @"闪传";
        _statusText = @"拖放到这里立即发送";
        [self registerForDraggedTypes:@[NSPasteboardTypeFileURL]];
        [self setAccessibilityRole:NSAccessibilityGroupRole];
        [self setAccessibilityLabel:@"闪传文件投放区"];
    }
    return self;
}

- (BOOL)isOpaque {
    return NO;
}

- (void)drawRect:(NSRect)dirtyRect {
    [super drawRect:dirtyRect];

    NSRect bounds = self.bounds;
    NSBezierPath *background = [NSBezierPath bezierPathWithRoundedRect:NSInsetRect(bounds, 1, 1)
                                                               xRadius:14
                                                               yRadius:14];
    NSColor *fill = self.dragHovering
                        ? [NSColor colorWithSRGBRed:0.10 green:0.45 blue:0.96 alpha:0.96]
                        : [NSColor colorWithWhite:0.10 alpha:0.94];
    [fill setFill];
    [background fill];
    [[NSColor colorWithWhite:1.0 alpha:self.dragHovering ? 0.72 : 0.18] setStroke];
    background.lineWidth = 1.5;
    [background stroke];

    NSImage *icon;
    if (@available(macOS 11.0, *)) {
        icon = [NSImage imageWithSystemSymbolName:@"paperplane.fill"
                        accessibilityDescription:@"发送文件"];
    } else {
        icon = [NSImage imageNamed:NSImageNameShareTemplate];
    }
    [icon setSize:NSMakeSize(34, 34)];
    [icon drawInRect:NSMakeRect(22, NSMidY(bounds) - 17, 34, 34)];

    NSColor *titleColor = NSColor.whiteColor;
    NSMutableParagraphStyle *titleStyle = [[NSMutableParagraphStyle alloc] init];
    titleStyle.lineBreakMode = NSLineBreakByTruncatingTail;
    NSDictionary *titleAttributes = @{
        NSFontAttributeName : [NSFont systemFontOfSize:15 weight:NSFontWeightSemibold],
        NSForegroundColorAttributeName : titleColor,
        NSParagraphStyleAttributeName : titleStyle,
    };
    [self.titleText drawInRect:NSMakeRect(70, NSHeight(bounds) - 45, NSWidth(bounds) - 90, 22)
                withAttributes:titleAttributes];

    NSColor *statusColor = self.targetAvailable
                               ? [NSColor colorWithWhite:1.0 alpha:0.72]
                               : [NSColor colorWithSRGBRed:1.0 green:0.47 blue:0.42 alpha:1.0];
    NSDictionary *statusAttributes = @{
        NSFontAttributeName : [NSFont systemFontOfSize:12 weight:NSFontWeightRegular],
        NSForegroundColorAttributeName : statusColor,
    };
    [self.statusText drawInRect:NSMakeRect(70, 20, NSWidth(bounds) - 90, 34)
                 withAttributes:statusAttributes];
}

- (NSDragOperation)draggingEntered:(id<NSDraggingInfo>)sender {
    NSArray<NSURL *> *urls = DBFileURLsFromPasteboard(sender.draggingPasteboard);
    self.dragHovering = urls.count > 0 && self.targetAvailable;
    [self setNeedsDisplay:YES];
    return self.dragHovering ? NSDragOperationCopy : NSDragOperationNone;
}

- (NSDragOperation)draggingUpdated:(id<NSDraggingInfo>)sender {
    return [self draggingEntered:sender];
}

- (void)draggingExited:(nullable id<NSDraggingInfo>)sender {
    self.dragHovering = NO;
    [self setNeedsDisplay:YES];
}

- (BOOL)prepareForDragOperation:(id<NSDraggingInfo>)sender {
    return self.targetAvailable &&
           DBFileURLsFromPasteboard(sender.draggingPasteboard).count > 0;
}

- (BOOL)performDragOperation:(id<NSDraggingInfo>)sender {
    NSArray<NSURL *> *urls = DBFileURLsFromPasteboard(sender.draggingPasteboard);
    if (!self.targetAvailable || urls.count == 0) {
        return NO;
    }
    self.dragHovering = NO;
    [self.controller handleDropURLs:urls];
    return YES;
}

@end

@implementation DBFlashDropController

- (void)configureWithLabel:(NSString *)label available:(BOOL)available {
    self.targetLabel = label.length > 0 ? label : @"未选择设备";
    self.targetAvailable = available;
    self.retainingResult = NO;
    self.resultGeneration += 1;

    if (self.panel == nil) {
        NSRect frame = NSMakeRect(0, 0, 330, 92);
        self.panel = [[DBFlashDropPanel alloc]
            initWithContentRect:frame
                      styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                        backing:NSBackingStoreBuffered
                          defer:NO];
        self.panel.opaque = NO;
        self.panel.backgroundColor = NSColor.clearColor;
        self.panel.hasShadow = YES;
        self.panel.hidesOnDeactivate = NO;
        self.panel.level = NSStatusWindowLevel;
        self.panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                        NSWindowCollectionBehaviorFullScreenAuxiliary |
                                        NSWindowCollectionBehaviorTransient;
        self.panel.releasedWhenClosed = NO;
        [self.panel setAccessibilityLabel:@"闪传文件投放窗口"];

        self.dropView = [[DBFlashDropView alloc] initWithFrame:frame];
        self.dropView.controller = self;
        self.panel.contentView = self.dropView;
    }
    [self refreshViewForIdleState];

    if (self.dragMonitor == nil) {
        __weak DBFlashDropController *weakSelf = self;
        self.dragMonitor = [NSEvent
            addGlobalMonitorForEventsMatchingMask:NSEventMaskLeftMouseDragged
                                          handler:^(NSEvent *event) {
                                            [weakSelf handleDragEvent:event];
                                          }];
        self.mouseUpMonitor = [NSEvent
            addGlobalMonitorForEventsMatchingMask:NSEventMaskLeftMouseUp
                                          handler:^(NSEvent *event) {
                                            (void)event;
                                            [weakSelf handleMouseUp];
                                          }];
        self.localMonitor = [NSEvent
            addLocalMonitorForEventsMatchingMask:NSEventMaskLeftMouseDragged | NSEventMaskLeftMouseUp
                                         handler:^NSEvent *(NSEvent *event) {
                                           if (event.type == NSEventTypeLeftMouseDragged) {
                                               [weakSelf handleDragEvent:event];
                                           } else {
                                               [weakSelf handleMouseUp];
                                           }
                                           return event;
                                         }];
    }
}

- (void)refreshViewForIdleState {
    self.dropView.titleText = [NSString stringWithFormat:@"闪传到 %@", self.targetLabel];
    self.dropView.targetAvailable = self.targetAvailable;
    self.dropView.dragHovering = NO;
    self.dropView.statusText = self.targetAvailable ? @"拖放到这里立即发送" : @"设备离线";
    [self.dropView setAccessibilityLabel:
                       [NSString stringWithFormat:@"闪传到 %@，%@",
                                                  self.targetLabel,
                                                  self.dropView.statusText]];
    [self.dropView setNeedsDisplay:YES];
}

- (NSScreen *)screenContainingPoint:(NSPoint)point {
    for (NSScreen *screen in NSScreen.screens) {
        if (NSPointInRect(point, screen.frame)) {
            return screen;
        }
    }
    return NSScreen.mainScreen;
}

- (void)handleDragEvent:(NSEvent *)event {
    if (self.retainingResult) {
        return;
    }

    NSArray<NSURL *> *urls = DBFileURLsFromPasteboard(
        [NSPasteboard pasteboardWithName:NSPasteboardNameDrag]);
    if (urls.count == 0) {
        [self.panel orderOut:nil];
        return;
    }

    NSPoint point = NSEvent.mouseLocation;
    NSScreen *screen = [self screenContainingPoint:point];
    if (screen == nil) {
        return;
    }
    NSRect visible = screen.visibleFrame;
    BOOL insideHotCorner = point.x >= NSMaxX(visible) - 132 &&
                           point.y >= NSMaxY(visible) - 112;
    BOOL insideVisiblePanel = self.panel.isVisible &&
                              NSPointInRect(point, NSInsetRect(self.panel.frame, -10, -10));
    if (!insideHotCorner && !insideVisiblePanel) {
        [self.panel orderOut:nil];
        return;
    }

    if (!self.panel.isVisible) {
        NSSize size = self.panel.frame.size;
        NSPoint origin = NSMakePoint(NSMaxX(visible) - size.width - 14,
                                     NSMaxY(visible) - size.height - 14);
        [self.panel setFrameOrigin:origin];
        [self refreshViewForIdleState];
        [self.panel orderFrontRegardless];
    }
}

- (void)handleMouseUp {
    // AppKit may deliver the global mouse-up immediately before it calls the
    // destination's performDragOperation:. Let that callback retain the panel.
    __weak DBFlashDropController *weakSelf = self;
    dispatch_async(dispatch_get_main_queue(), ^{
      DBFlashDropController *strongSelf = weakSelf;
      if (strongSelf != nil && !strongSelf.retainingResult) {
          [strongSelf.panel orderOut:nil];
      }
    });
}

- (void)handleDropURLs:(NSArray<NSURL *> *)urls {
    self.resultGeneration += 1;
    self.retainingResult = YES;
    self.dropView.statusText = @"正在发送…";
    [self.dropView setNeedsDisplay:YES];

    if (gFilesCallback == NULL) {
        [self showResult:NO message:@"发送功能不可用"];
        return;
    }

    size_t count = urls.count;
    const char **paths = calloc(count, sizeof(char *));
    if (paths == NULL) {
        [self showResult:NO message:@"无法读取文件"];
        return;
    }
    for (size_t index = 0; index < count; index++) {
        paths[index] = urls[index].path.fileSystemRepresentation;
    }
    gFilesCallback(paths, count);
    free(paths);
}

- (void)showResult:(BOOL)success message:(NSString *)message {
    self.resultGeneration += 1;
    NSUInteger generation = self.resultGeneration;
    self.retainingResult = YES;
    self.dropView.dragHovering = success;
    self.dropView.statusText = message.length > 0
                                   ? message
                                   : (success ? @"发送完成" : @"发送失败");
    [self.dropView setAccessibilityLabel:
                       [NSString stringWithFormat:@"闪传到 %@，%@",
                                                  self.targetLabel,
                                                  self.dropView.statusText]];
    [self.dropView setNeedsDisplay:YES];

    __weak DBFlashDropController *weakSelf = self;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(2.2 * NSEC_PER_SEC)),
                   dispatch_get_main_queue(), ^{
                     DBFlashDropController *strongSelf = weakSelf;
                     if (strongSelf == nil) {
                         return;
                     }
                     if (strongSelf.resultGeneration != generation ||
                         !strongSelf.retainingResult) {
                         return;
                     }
                     strongSelf.retainingResult = NO;
                     [strongSelf.panel orderOut:nil];
                     [strongSelf refreshViewForIdleState];
                   });
}

- (void)stop {
    self.resultGeneration += 1;
    if (self.dragMonitor != nil) {
        [NSEvent removeMonitor:self.dragMonitor];
        self.dragMonitor = nil;
    }
    if (self.mouseUpMonitor != nil) {
        [NSEvent removeMonitor:self.mouseUpMonitor];
        self.mouseUpMonitor = nil;
    }
    if (self.localMonitor != nil) {
        [NSEvent removeMonitor:self.localMonitor];
        self.localMonitor = nil;
    }
    [self.panel orderOut:nil];
    [self.dropView unregisterDraggedTypes];
    self.dropView.controller = nil;
    self.dropView = nil;
    self.panel = nil;
    self.retainingResult = NO;
}

@end

void dashbeam_flash_drop_configure(const char *target_label,
                                   bool available,
                                   DBFlashDropFilesCallback callback) {
    NSCAssert(NSThread.isMainThread, @"Flash Drop must be configured on the main thread");
    NSString *label = target_label != NULL
                          ? [NSString stringWithUTF8String:target_label]
                          : @"";
    gFilesCallback = callback;
    if (gController == nil) {
        gController = [[DBFlashDropController alloc] init];
    }
    [gController configureWithLabel:label available:available];
}

void dashbeam_flash_drop_stop(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
      [gController stop];
      gController = nil;
      gFilesCallback = NULL;
    });
}

void dashbeam_flash_drop_show_result(bool success, const char *message) {
    NSString *copiedMessage = message != NULL
                                  ? [[NSString alloc] initWithUTF8String:message]
                                  : @"";
    dispatch_async(dispatch_get_main_queue(), ^{
      [gController showResult:success message:copiedMessage];
    });
}
