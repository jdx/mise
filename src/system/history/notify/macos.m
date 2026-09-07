// Embedded at build time: users need no compiler or third-party notifier.
#import <AppKit/AppKit.h>
#import <CoreServices/CoreServices.h>
#import <UserNotifications/UserNotifications.h>
#include <string.h>

static NSString *const notificationID = @"mise.dotfiles.sync";

@interface MiseNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation MiseNotificationDelegate
- (void)userNotificationCenter:(UNUserNotificationCenter *)center
      willPresentNotification:(UNNotification *)notification
        withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completion {
    completion(UNNotificationPresentationOptionAlert);
}
@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        // Clicking an old alert may relaunch the app. Never send it again.
        if (argc == 1) return 0;
        NSBundle *bundle = NSBundle.mainBundle;
        if (argc == 2 && strcmp(argv[1], "--check") == 0) {
            NSString *icon = [bundle pathForResource:@"mise" ofType:@"icns"];
            return [bundle.bundleIdentifier isEqualToString:@"dev.jdx.mise.notifications"] &&
                [[NSImage alloc] initWithContentsOfFile:icon] != nil ? 0 : 2;
        }
        if (argc != 3) return 2;
        // Register our own bundle so Notification Center can resolve its name
        // and icon even when mise invokes the embedded executable directly.
        if (LSRegisterURL((__bridge CFURLRef)bundle.bundleURL, true) != noErr) return 6;
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
        UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
        NSString *title = [NSString stringWithUTF8String:argv[1]];
        NSString *body = [NSString stringWithUTF8String:argv[2]];
        if (title == nil || body == nil) return 2;
        MiseNotificationDelegate *delegate = [MiseNotificationDelegate new];
        center.delegate = delegate;
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        __block int result = 4;
        [center requestAuthorizationWithOptions:UNAuthorizationOptionAlert
            completionHandler:^(BOOL granted, NSError *error) {
                if (!granted || error != nil) {
                    result = 3;
                    dispatch_semaphore_signal(done);
                    return;
                }
                UNMutableNotificationContent *content = [UNMutableNotificationContent new];
                content.title = title;
                content.body = body;
                UNNotificationRequest *request = [UNNotificationRequest
                    requestWithIdentifier:notificationID content:content trigger:nil];
                [center addNotificationRequest:request withCompletionHandler:^(NSError *failure) {
                    result = failure == nil ? 0 : 5;
                    dispatch_semaphore_signal(done);
                }];
            }];
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:30];
        while (dispatch_semaphore_wait(done, DISPATCH_TIME_NOW) != 0) {
            if (deadline.timeIntervalSinceNow <= 0) return 4;
            [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
        }
        return result;
    }
}
