#import <AppKit/AppKit.h>
#import <UserNotifications/UserNotifications.h>
#include <stdint.h>

typedef void (*DBNotificationCompletion)(uint64_t, const char *);
typedef void (*DBNotificationOpen)(void);

static NSString *const DBFilesCategory = @"dashbeam.received-files";
static NSString *const DBCopyFiles = @"dashbeam.copy-files";
static NSString *const DBCopyPaths = @"dashbeam.copy-paths";
static DBNotificationOpen gOpenReceivedFiles;

// Validate the entire selection before changing the clipboard. In particular,
// never fall back to the download directory when a received file was removed.
static int DBCopyReceivedFilesToPasteboard(NSArray *paths, BOOL asPaths, NSPasteboard *pasteboard) {
    if (![paths isKindOfClass:[NSArray class]] || paths.count == 0) return 1;
    NSMutableArray<NSURL *> *urls = [NSMutableArray arrayWithCapacity:paths.count];
    for (id path in paths) {
        if (![path isKindOfClass:[NSString class]] || ![path isAbsolutePath] ||
            ![[NSFileManager defaultManager] isReadableFileAtPath:path]) return 1;
        [urls addObject:[NSURL fileURLWithPath:path]];
    }
    [pasteboard clearContents];
    if (asPaths) {
        return [pasteboard setString:[paths componentsJoinedByString:@"\n"]
                             forType:NSPasteboardTypeString] ? 0 : 2;
    }
    // NSURL supplies public.file-url so Finder sees files, not a text listing.
    if (![pasteboard writeObjects:urls]) return 2;
    // Older applications still consume the filenames property list.
    [pasteboard setPropertyList:paths forType:@"NSFilenamesPboardType"];
    return 0;
}

static int DBCopyReceivedFiles(NSArray *paths, BOOL asPaths) {
    return DBCopyReceivedFilesToPasteboard(paths, asPaths, [NSPasteboard generalPasteboard]);
}

int dashbeam_copy_received_files(const char *json, bool as_paths) {
    @autoreleasepool {
        if (![NSThread isMainThread] || !json) return 2;
        NSData *data = [[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding];
        return DBCopyReceivedFiles([NSJSONSerialization JSONObjectWithData:data options:0 error:nil], as_paths);
    }
}

API_AVAILABLE(macos(10.14))
@interface DBReceivedFilesNotifications : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation DBReceivedFilesNotifications
- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completionHandler {
    (void)center;
    (void)notification;
    if (@available(macOS 11.0, *)) {
        completionHandler(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList);
    } else {
        completionHandler(UNNotificationPresentationOptionAlert);
    }
}

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
didReceiveNotificationResponse:(UNNotificationResponse *)response
         withCompletionHandler:(void (^)(void))completionHandler {
    // Every notification owns its paths. A later receive must not change what
    // an older notification's Copy action refers to, including after relaunch.
    NSDictionary *info = response.notification.request.content.userInfo;
    NSString *action = response.actionIdentifier;
    dispatch_async(dispatch_get_main_queue(), ^{
        if ([action isEqualToString:DBCopyFiles] || [action isEqualToString:DBCopyPaths]) {
            int result = DBCopyReceivedFiles(info[@"paths"], [action isEqualToString:DBCopyPaths]);
            if (result != 0) {
                NSDictionary *labels = info[@"labels"];
                UNMutableNotificationContent *content = [UNMutableNotificationContent new];
                content.title = labels[@"copyFailed"] ?: @"Could not copy received files";
                content.body = result == 1
                    ? (labels[@"missingFiles"] ?: @"A received file was moved, removed, or is no longer readable.")
                    : (labels[@"copyFailed"] ?: @"Could not write to the clipboard.");
                UNNotificationRequest *request = [UNNotificationRequest
                    requestWithIdentifier:[@"dashbeam.copy-error." stringByAppendingString:NSUUID.UUID.UUIDString]
                    content:content trigger:nil];
                [center addNotificationRequest:request withCompletionHandler:nil];
            }
        } else if ([action isEqualToString:UNNotificationDefaultActionIdentifier] && gOpenReceivedFiles) {
            gOpenReceivedFiles();
        }
        completionHandler();
    });
}
@end

static DBReceivedFilesNotifications *gNotifications API_AVAILABLE(macos(10.14));

bool dashbeam_receive_notifications_available(void) {
    if (@available(macOS 10.14, *)) {
        NSBundle *bundle = [NSBundle mainBundle];
        return [bundle.bundleURL.pathExtension isEqualToString:@"app"] && bundle.bundleIdentifier.length > 0;
    }
    return false;
}

void dashbeam_receive_notifications_init(DBNotificationOpen open_callback) {
    NSCAssert([NSThread isMainThread], @"Notification initialization requires the main thread");
    gOpenReceivedFiles = open_callback;
    // currentNotificationCenter may throw an Objective-C exception for a bare
    // `tauri dev`/CLI executable. Such runs keep their ordinary notifications.
    if (!dashbeam_receive_notifications_available()) return;
    if (@available(macOS 10.14, *)) {
        if (!gNotifications) gNotifications = [DBReceivedFilesNotifications new];
        [UNUserNotificationCenter currentNotificationCenter].delegate = gNotifications;
    }
}

static void DBShowReceiveNotification(const char *json, uint64_t request_id,
                                     DBNotificationCompletion callback) API_AVAILABLE(macos(10.14));

static void DBShowReceiveNotification(const char *json, uint64_t request_id,
                                     DBNotificationCompletion callback) {
    @autoreleasepool {
        NSCAssert([NSThread isMainThread], @"Notification scheduling requires the main thread");
        if (!dashbeam_receive_notifications_available()) {
            callback(request_id, "Actionable notifications require an application bundle");
            return;
        }
        NSData *data = [[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding];
        NSDictionary *payload = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
        if (![payload isKindOfClass:[NSDictionary class]]) {
            callback(request_id, "Invalid received-file notification");
            return;
        }
        NSDictionary *labels = payload[@"labels"];
        UNNotificationAction *files = [UNNotificationAction actionWithIdentifier:DBCopyFiles
            title:labels[@"copyFiles"] options:UNNotificationActionOptionNone];
        UNNotificationAction *paths = [UNNotificationAction actionWithIdentifier:DBCopyPaths
            title:labels[@"copyPaths"] options:UNNotificationActionOptionNone];
        UNNotificationCategory *category = [UNNotificationCategory categoryWithIdentifier:DBFilesCategory
            actions:@[files, paths] intentIdentifiers:@[] options:UNNotificationCategoryOptionNone];
        UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
        [center setNotificationCategories:[NSSet setWithObject:category]];

        UNMutableNotificationContent *content = [UNMutableNotificationContent new];
        content.title = payload[@"title"];
        content.body = [payload[@"body"] isKindOfClass:[NSString class]] ? payload[@"body"] : @"";
        content.categoryIdentifier = DBFilesCategory;
        content.threadIdentifier = @"dashbeam.receives";
        content.userInfo = @{@"paths": payload[@"paths"], @"labels": labels};
        UNNotificationRequest *request = [UNNotificationRequest
            requestWithIdentifier:[@"dashbeam.receive." stringByAppendingString:NSUUID.UUID.UUIDString]
            content:content trigger:nil];

        // The desktop Tauri plugin reports Granted without querying macOS.
        // Request real authorization here; no worker waits or polling timers.
        [center requestAuthorizationWithOptions:(UNAuthorizationOptionAlert | UNAuthorizationOptionSound)
            completionHandler:^(BOOL granted, NSError *error) {
                if (!granted || error) {
                    callback(request_id, "Notification permission is not granted");
                    return;
                }
                [center addNotificationRequest:request withCompletionHandler:^(NSError *scheduleError) {
                    callback(request_id, scheduleError ? "Could not schedule received-file notification" : NULL);
                }];
            }];
    }
}

void dashbeam_receive_notification_show(const char *json, uint64_t request_id,
                                      DBNotificationCompletion callback) {
    if (@available(macOS 10.14, *)) {
        DBShowReceiveNotification(json, request_id, callback);
    } else {
        callback(request_id, "Actionable notifications require macOS 10.14 or later");
    }
}
