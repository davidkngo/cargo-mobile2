#include "bindings/bindings.h"

#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>
#import <QuartzCore/QuartzCore.h>
#import <objc/runtime.h>

// ---------------------------------------------------------------------------
// Suppress Qt's iOS keyboard-avoidance view translation.
//
// Qt's platform plugin (QIOSInputContext::scroll) does keyboard avoidance by
// translating the ENTIRE app: it sets `sublayerTransform` on the root view's
// layer (window.rootViewController.view, a QIOSViewController) AND drives it with
// an explicit CABasicAnimation. That fights per-widget keyboard handling done in
// QML (e.g. growing a sheet's bottom padding by the keyboard height) and causes
// the whole view to scroll up and snap back.
//
// The keyboard *rectangle* QML reads comes from a separate path
// (updateKeyboardState), so refusing the translation does NOT break keyboard-size
// reporting.
//
// Fix: swizzle the two CALayer entry points Qt uses and, ONLY for the layer that
// backs the QIOSViewController root view, refuse the translation (force identity)
// and drop the scroll animation. Installed at the very start of main(), before Qt
// runs, so it's in place before the first keyboard event. Every other layer is
// untouched.
// ---------------------------------------------------------------------------

static void (*g_orig_setSublayerTransform)(id, SEL, CATransform3D) = NULL;
static void (*g_orig_addAnimation)(id, SEL, CAAnimation *, NSString *) = NULL;

static BOOL isQtRootViewLayer(CALayer *layer) {
    id delegate = layer.delegate; // a view's backing layer has the view as delegate
    if (![delegate isKindOfClass:[UIView class]])
        return NO;

    static Class vcClass = Nil;
    static dispatch_once_t once;
    dispatch_once(&once, ^{ vcClass = NSClassFromString(@"QIOSViewController"); });
    if (vcClass == Nil)
        return NO;

    // A UIViewController's view has the controller as its nextResponder.
    return [[(UIView *)delegate nextResponder] isKindOfClass:vcClass];
}

static void swizzled_setSublayerTransform(id self, SEL _cmd, CATransform3D t) {
    if (isQtRootViewLayer((CALayer *)self)) {
        g_orig_setSublayerTransform(self, _cmd, CATransform3DIdentity);
        return;
    }
    g_orig_setSublayerTransform(self, _cmd, t);
}

static void swizzled_addAnimation(id self, SEL _cmd, CAAnimation *anim, NSString *key) {
    if (isQtRootViewLayer((CALayer *)self)) {
        BOOL isSublayerXform = [key isEqualToString:@"AnimateSubLayerTransform"];
        if (!isSublayerXform && [anim isKindOfClass:[CAPropertyAnimation class]]) {
            isSublayerXform =
                [[(CAPropertyAnimation *)anim keyPath] isEqualToString:@"sublayerTransform"];
        }
        if (isSublayerXform)
            return; // drop Qt's keyboard-avoidance scroll animation
    }
    g_orig_addAnimation(self, _cmd, anim, key);
}

static void installKeyboardScrollFix(void) {
    Method m1 = class_getInstanceMethod([CALayer class], @selector(setSublayerTransform:));
    if (m1) {
        g_orig_setSublayerTransform =
            (void (*)(id, SEL, CATransform3D))method_getImplementation(m1);
        method_setImplementation(m1, (IMP)swizzled_setSublayerTransform);
    }
    Method m2 = class_getInstanceMethod([CALayer class], @selector(addAnimation:forKey:));
    if (m2) {
        g_orig_addAnimation =
            (void (*)(id, SEL, CAAnimation *, NSString *))method_getImplementation(m2);
        method_setImplementation(m2, (IMP)swizzled_addAnimation);
    }
}

int main(int argc, char * argv[]) {
	installKeyboardScrollFix();
	ffi::start_app();
	return 0;
}
