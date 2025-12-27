# JNI Android Integration

## Overview
Java Native Interface (JNI) patterns for accessing Android APIs from Rust code.

## When to Use
- Accessing Android system services (UsbManager, etc.)
- Calling Java/Kotlin APIs from Rust
- Handling Android permissions
- Interacting with the Activity lifecycle

## Key Concepts

### Getting JVM and Activity Context
```rust
use ndk_context;

fn get_android_context() -> Result<(*mut jni::sys::JavaVM, jni::objects::GlobalRef), String> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| format!("Failed to get VM: {}", e))?;

    let activity = unsafe {
        jni::objects::JObject::from_raw(ctx.context().cast())
    };

    let env = vm.attach_current_thread()
        .map_err(|e| format!("Failed to attach thread: {}", e))?;

    let global_ref = env.new_global_ref(activity)
        .map_err(|e| format!("Failed to create global ref: {}", e))?;

    Ok((ctx.vm().cast(), global_ref))
}
```

### Thread Attachment Pattern
JNI calls must be made from an attached thread:
```rust
fn with_jni<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut jni::JNIEnv, jni::objects::JObject) -> Result<R, String>,
{
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| e.to_string())?;

    let mut env = vm.attach_current_thread()
        .map_err(|e| e.to_string())?;

    let activity = unsafe {
        jni::objects::JObject::from_raw(ctx.context().cast())
    };

    f(&mut env, activity)
}
```

### Type Signatures
JNI uses encoded type signatures:

| Java Type | JNI Signature |
|-----------|---------------|
| `void` | `V` |
| `boolean` | `Z` |
| `int` | `I` |
| `long` | `J` |
| `String` | `Ljava/lang/String;` |
| `Object` | `Ljava/lang/Object;` |
| `int[]` | `[I` |
| `Object[]` | `[Ljava/lang/Object;` |

Method signature format: `(parameters)return_type`

Example: `(Ljava/lang/String;I)Z` = `boolean method(String, int)`

## Common Patterns

### Getting System Service (UsbManager)
```rust
fn get_usb_manager(
    env: &mut jni::JNIEnv,
    activity: jni::objects::JObject,
) -> Result<jni::objects::JObject, String> {
    let usb_service = env.new_string("usb")
        .map_err(|e| e.to_string())?;

    let manager = env.call_method(
        activity,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[(&usb_service).into()],
    )
    .map_err(|e| e.to_string())?
    .l()
    .map_err(|e| e.to_string())?;

    Ok(manager)
}
```

### Opening USB Device
```rust
fn open_usb_device(
    env: &mut jni::JNIEnv,
    usb_manager: jni::objects::JObject,
    usb_device: jni::objects::JObject,
) -> Result<i32, String> {
    // Open connection
    let connection = env.call_method(
        usb_manager,
        "openDevice",
        "(Landroid/hardware/usb/UsbDevice;)Landroid/hardware/usb/UsbDeviceConnection;",
        &[(&usb_device).into()],
    )
    .map_err(|e| e.to_string())?
    .l()
    .map_err(|e| e.to_string())?;

    if connection.is_null() {
        return Err("Failed to open USB device".to_string());
    }

    // Get file descriptor
    let fd = env.call_method(
        connection,
        "getFileDescriptor",
        "()I",
        &[],
    )
    .map_err(|e| e.to_string())?
    .i()
    .map_err(|e| e.to_string())?;

    Ok(fd)
}
```

### Checking Permissions
```rust
fn has_usb_permission(
    env: &mut jni::JNIEnv,
    usb_manager: jni::objects::JObject,
    usb_device: jni::objects::JObject,
) -> Result<bool, String> {
    let has_perm = env.call_method(
        usb_manager,
        "hasPermission",
        "(Landroid/hardware/usb/UsbDevice;)Z",
        &[(&usb_device).into()],
    )
    .map_err(|e| e.to_string())?
    .z()
    .map_err(|e| e.to_string())?;

    Ok(has_perm)
}
```

### Iterating USB Devices
```rust
fn get_usb_devices(
    env: &mut jni::JNIEnv,
    usb_manager: jni::objects::JObject,
) -> Result<Vec<jni::objects::GlobalRef>, String> {
    let device_map = env.call_method(
        usb_manager,
        "getDeviceList",
        "()Ljava/util/HashMap;",
        &[],
    )
    .map_err(|e| e.to_string())?
    .l()
    .map_err(|e| e.to_string())?;

    let values = env.call_method(
        device_map,
        "values",
        "()Ljava/util/Collection;",
        &[],
    )
    .map_err(|e| e.to_string())?
    .l()
    .map_err(|e| e.to_string())?;

    let iterator = env.call_method(
        values,
        "iterator",
        "()Ljava/util/Iterator;",
        &[],
    )
    .map_err(|e| e.to_string())?
    .l()
    .map_err(|e| e.to_string())?;

    let mut devices = Vec::new();

    loop {
        let has_next = env.call_method(
            &iterator,
            "hasNext",
            "()Z",
            &[],
        )
        .map_err(|e| e.to_string())?
        .z()
        .map_err(|e| e.to_string())?;

        if !has_next {
            break;
        }

        let device = env.call_method(
            &iterator,
            "next",
            "()Ljava/lang/Object;",
            &[],
        )
        .map_err(|e| e.to_string())?
        .l()
        .map_err(|e| e.to_string())?;

        let global_ref = env.new_global_ref(device)
            .map_err(|e| e.to_string())?;
        devices.push(global_ref);
    }

    Ok(devices)
}
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| `NoSuchMethodError` | Check method signature is correct |
| `ClassNotFoundException` | Use fully qualified class name |
| `NullPointerException` | Check for null returns from JNI calls |
| Thread crash | Ensure thread is attached before JNI calls |
| Memory leak | Use `DeleteLocalRef` or global refs appropriately |

## References
- [JNI Documentation](https://docs.oracle.com/javase/8/docs/technotes/guides/jni/)
- [Android JNI Tips](https://developer.android.com/training/articles/perf-jni)
- [jni-rs Crate](https://docs.rs/jni/)
- [ndk-context](https://docs.rs/ndk-context/)
