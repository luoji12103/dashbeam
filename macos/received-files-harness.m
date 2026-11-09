// Exercises the real file-pasteboard writer on a private pasteboard. Neither
// the user's clipboard nor Notification Center permissions are touched.
#import "../src-tauri/native/received_files.m"

static void require(BOOL condition, NSString *message) {
    if (!condition) {
        fprintf(stderr, "%s\n", message.UTF8String);
        exit(1);
    }
}

int main(void) {
    @autoreleasepool {
        require(!dashbeam_receive_notifications_available(), @"bare CLI skips UN notification initialization");
        dashbeam_receive_notifications_init(NULL);
        NSFileManager *fm = [NSFileManager defaultManager];
        NSString *root = [NSTemporaryDirectory() stringByAppendingPathComponent:
            [@"dashbeam-clipboard-" stringByAppendingString:NSUUID.UUID.UUIDString]];
        require([fm createDirectoryAtPath:root withIntermediateDirectories:YES attributes:nil error:nil], @"fixture directory");
        NSArray *paths = @[[root stringByAppendingPathComponent:@"中文 notes (1).md"],
                           [root stringByAppendingPathComponent:@"spaces & #.txt"]];
        for (NSString *path in paths) {
            require([@"fixture" writeToFile:path atomically:YES encoding:NSUTF8StringEncoding error:nil], @"fixture file");
        }
        NSPasteboard *board = [NSPasteboard pasteboardWithUniqueName];
        require(DBCopyReceivedFilesToPasteboard(paths, NO, board) == 0, @"copy files");
        NSArray<NSURL *> *urls = [board readObjectsForClasses:@[[NSURL class]]
            options:@{NSPasteboardURLReadingFileURLsOnlyKey: @YES}];
        require(urls.count == 2, @"file URL item count");
        require([urls[0].path isEqual:paths[0]] && [urls[1].path isEqual:paths[1]], @"exact Unicode and conflict-resolved filenames");
        require([[board propertyListForType:@"NSFilenamesPboardType"] isEqual:paths], @"legacy filenames");

        require(DBCopyReceivedFilesToPasteboard(paths, YES, board) == 0, @"copy paths");
        NSString *expected = [paths componentsJoinedByString:@"\n"];
        require([[board stringForType:NSPasteboardTypeString] isEqual:expected], @"newline-separated actual paths");
        require([fm removeItemAtPath:paths[1] error:nil], @"remove fixture");
        NSInteger version = board.changeCount;
        require(DBCopyReceivedFilesToPasteboard(paths, NO, board) == 1, @"reject missing file");
        require(board.changeCount == version && [[board stringForType:NSPasteboardTypeString] isEqual:expected], @"missing file preserves clipboard");
        require(DBCopyReceivedFilesToPasteboard(@[@"relative.txt"], NO, board) == 1, @"reject relative file");
        require(DBCopyReceivedFilesToPasteboard(@[], NO, board) == 1, @"reject empty receive");
        [board releaseGlobally];
        require([fm removeItemAtPath:root error:nil], @"clean fixture directory");
        puts("PASS: file URLs, multi-file paths, renamed Unicode files, missing-file clipboard preservation");
    }
    return 0;
}
