// This file contains the C bindings that are exported when building
// the system libraries.
//
// WHERE IS THE DOCUMENTATION? Note that all the documentation for the C
// interface is in the man pages. The header file xev.h purposely has no
// documentation so that its concise and easy to see the list of exported
// functions.

const std = @import("std");
const builtin = @import("builtin");
const assert = std.debug.assert;
const xev = @import("main.zig");

const func_callconv: std.builtin.CallingConvention = if (blk: {
    const order = builtin.zig_version.order(.{ .major = 0, .minor = 14, .patch = 1 });
    break :blk order == .lt or order == .eq;
}) .C else .c;

export fn xev_loop_init(loop: *xev.Loop) c_int {
    // TODO: overflow
    loop.* = xev.Loop.init(.{}) catch |err| return errorCode(err);
    return 0;
}

export fn xev_loop_deinit(loop: *xev.Loop) void {
    loop.deinit();
}

export fn xev_loop_run(loop: *xev.Loop, mode: xev.RunMode) c_int {
    loop.run(mode) catch |err| return errorCode(err);
    return 0;
}

export fn xev_loop_now(loop: *xev.Loop) i64 {
    return loop.now();
}

export fn xev_loop_update_now(loop: *xev.Loop) void {
    loop.update_now();
}

export fn xev_completion_zero(c: *xev.Completion) void {
    c.* = .{};
}

export fn xev_completion_state(c: *xev.Completion) xev.CompletionState {
    return c.state();
}

//-------------------------------------------------------------------
// ThreadPool

export fn xev_threadpool_config_init(cfg: *xev.ThreadPool.Config) void {
    cfg.* = .{};
}

export fn xev_threadpool_config_set_stack_size(
    cfg: *xev.ThreadPool.Config,
    v: u32,
) void {
    cfg.stack_size = v;
}

export fn xev_threadpool_config_set_max_threads(
    cfg: *xev.ThreadPool.Config,
    v: u32,
) void {
    cfg.max_threads = v;
}

export fn xev_threadpool_init(
    threadpool: *xev.ThreadPool,
    cfg_: ?*xev.ThreadPool.Config,
) c_int {
    const cfg: xev.ThreadPool.Config = if (cfg_) |v| v.* else .{};
    threadpool.* = xev.ThreadPool.init(cfg);
    return 0;
}

export fn xev_threadpool_deinit(threadpool: *xev.ThreadPool) void {
    threadpool.deinit();
}

export fn xev_threadpool_shutdown(threadpool: *xev.ThreadPool) void {
    threadpool.shutdown();
}

export fn xev_threadpool_schedule(
    pool: *xev.ThreadPool,
    batch: *xev.ThreadPool.Batch,
) void {
    pool.schedule(batch.*);
}

export fn xev_threadpool_task_init(
    t: *xev.ThreadPool.Task,
    cb: *const fn (*xev.ThreadPool.Task) callconv(func_callconv) void,
) void {
    const extern_t = @as(*Task, @ptrCast(@alignCast(t)));
    extern_t.c_callback = cb;

    t.* = .{
        .callback = (struct {
            fn callback(inner_t: *xev.ThreadPool.Task) void {
                const outer_t: *Task = @alignCast(@fieldParentPtr(
                    "data",
                    @as(*Task.Data, @ptrCast(inner_t)),
                ));
                outer_t.c_callback(inner_t);
            }
        }).callback,
    };
}

export fn xev_threadpool_batch_init(b: *xev.ThreadPool.Batch) void {
    b.* = .{};
}

export fn xev_threadpool_batch_push_task(
    b: *xev.ThreadPool.Batch,
    t: *xev.ThreadPool.Task,
) void {
    b.push(xev.ThreadPool.Batch.from(t));
}

export fn xev_threadpool_batch_push_batch(
    b: *xev.ThreadPool.Batch,
    other: *xev.ThreadPool.Batch,
) void {
    b.push(other.*);
}

//-------------------------------------------------------------------
// Timers

export fn xev_timer_init(v: *xev.Timer) c_int {
    v.* = xev.Timer.init() catch |err| return errorCode(err);
    return 0;
}

export fn xev_timer_deinit(v: *xev.Timer) void {
    v.deinit();
}

export fn xev_timer_run(
    v: *xev.Timer,
    loop: *xev.Loop,
    c: *xev.Completion,
    next_ms: u64,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        c_int,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.run(loop, c, next_ms, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            r: xev.Timer.RunError!void,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                if (r) |_| 0 else |err| errorCode(err),
                ud,
            });
        }
    }).callback);
}

export fn xev_timer_reset(
    v: *xev.Timer,
    loop: *xev.Loop,
    c: *xev.Completion,
    c_cancel: *xev.Completion,
    next_ms: u64,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        c_int,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.reset(loop, c, c_cancel, next_ms, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            r: xev.Timer.RunError!void,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                if (r) |_| 0 else |err| errorCode(err),
                ud,
            });
        }
    }).callback);
}

export fn xev_timer_cancel(
    v: *xev.Timer,
    loop: *xev.Loop,
    c_timer: *xev.Completion,
    c_cancel: *xev.Completion,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        c_int,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c_cancel)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.cancel(loop, c_timer, c_cancel, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            r: xev.Timer.CancelError!void,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                if (r) |_| 0 else |err| errorCode(err),
                ud,
            });
        }
    }).callback);
}

//-------------------------------------------------------------------
// Async

export fn xev_async_init(v: *xev.Async) c_int {
    v.* = xev.Async.init() catch |err| return errorCode(err);
    return 0;
}

export fn xev_async_deinit(v: *xev.Async) void {
    v.deinit();
}

export fn xev_async_notify(v: *xev.Async) c_int {
    v.notify() catch |err| return errorCode(err);
    return 0;
}

export fn xev_async_wait(
    v: *xev.Async,
    loop: *xev.Loop,
    c: *xev.Completion,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        c_int,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.wait(loop, c, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            r: xev.Async.WaitError!void,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                if (r) |_| 0 else |err| errorCode(err),
                ud,
            });
        }
    }).callback);
}

//-------------------------------------------------------------------
// File

export fn xev_file_init(v: *xev.File, fd: usize) c_int {
    // std.Io.File.Handle is an integer fd on POSIX but a HANDLE
    // (`*anyopaque`) on Windows. Accept the handle as a uintptr-sized
    // value so the C ABI is wide enough for a 64-bit Windows HANDLE,
    // then convert to whatever the platform's Handle type actually is.
    const Handle = std.Io.File.Handle;
    const handle: Handle = switch (@typeInfo(Handle)) {
        .int => @intCast(@as(isize, @bitCast(fd))),
        .pointer => @ptrFromInt(fd),
        else => @compileError("unsupported std.Io.File.Handle type"),
    };
    v.* = xev.File.initFd(handle);
    return 0;
}

export fn xev_file_deinit(v: *xev.File) void {
    v.deinit();
}

export fn xev_file_close(
    v: *xev.File,
    loop: *xev.Loop,
    c: *xev.Completion,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        c_int,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.close(loop, c, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            _: xev.File,
            r: xev.CloseError!void,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                if (r) |_| 0 else |err| errorCode(err),
                ud,
            });
        }
    }).callback);
}

/// Encode a read/write result. >= 0 means bytes transferred,
/// < 0 means -errorCode(err).
fn rwResult(comptime E: type, r: E!usize) isize {
    if (r) |v_ok| {
        return @intCast(v_ok);
    } else |err| {
        return -@as(isize, @intCast(errorCode(err)));
    }
}

export fn xev_file_read(
    v: *xev.File,
    loop: *xev.Loop,
    c: *xev.Completion,
    buf: [*]u8,
    len: usize,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        isize,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.read(loop, c, .{ .slice = buf[0..len] }, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            _: xev.File,
            _: xev.ReadBuffer,
            r: xev.ReadError!usize,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                rwResult(xev.ReadError, r),
                ud,
            });
        }
    }).callback);
}

export fn xev_file_pread(
    v: *xev.File,
    loop: *xev.Loop,
    c: *xev.Completion,
    buf: [*]u8,
    len: usize,
    offset: u64,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        isize,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.pread(loop, c, .{ .slice = buf[0..len] }, offset, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            _: xev.File,
            _: xev.ReadBuffer,
            r: xev.ReadError!usize,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                rwResult(xev.ReadError, r),
                ud,
            });
        }
    }).callback);
}

export fn xev_file_write(
    v: *xev.File,
    loop: *xev.Loop,
    c: *xev.Completion,
    buf: [*]const u8,
    len: usize,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        isize,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.write(loop, c, .{ .slice = buf[0..len] }, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            _: xev.File,
            _: xev.WriteBuffer,
            r: xev.WriteError!usize,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                rwResult(xev.WriteError, r),
                ud,
            });
        }
    }).callback);
}

export fn xev_file_pwrite(
    v: *xev.File,
    loop: *xev.Loop,
    c: *xev.Completion,
    buf: [*]const u8,
    len: usize,
    offset: u64,
    userdata: ?*anyopaque,
    cb: *const fn (
        *xev.Loop,
        *xev.Completion,
        isize,
        ?*anyopaque,
    ) callconv(func_callconv) xev.CallbackAction,
) void {
    const Callback = @typeInfo(@TypeOf(cb)).pointer.child;
    const extern_c = @as(*Completion, @ptrCast(@alignCast(c)));
    extern_c.c_callback = @as(*const anyopaque, @ptrCast(cb));

    v.pwrite(loop, c, .{ .slice = buf[0..len] }, offset, anyopaque, userdata, (struct {
        fn callback(
            ud: ?*anyopaque,
            cb_loop: *xev.Loop,
            cb_c: *xev.Completion,
            _: xev.File,
            _: xev.WriteBuffer,
            r: xev.WriteError!usize,
        ) xev.CallbackAction {
            const cb_extern_c = @as(*Completion, @ptrCast(cb_c));
            const cb_c_callback = @as(
                *const Callback,
                @ptrCast(@alignCast(cb_extern_c.c_callback)),
            );
            return @call(.auto, cb_c_callback, .{
                cb_loop,
                cb_c,
                rwResult(xev.WriteError, r),
                ud,
            });
        }
    }).callback);
}

//-------------------------------------------------------------------
// Sync with xev.h

/// Since we can't pass the callback at comptime with C, we have to
/// have an additional field on completions to store our callback pointer.
/// We just tack it onto the end of the memory chunk that C programs allocate
/// for completions.
const Completion = extern struct {
    const Data = [@sizeOf(xev.Completion)]u8;
    data: Data,
    c_callback: *const anyopaque,
};

const Task = extern struct {
    const Data = [@sizeOf(xev.ThreadPool.Task)]u8;
    data: Data,
    c_callback: *const fn (*xev.ThreadPool.Task) callconv(func_callconv) void,
};

/// Returns the unique error code for an error.
fn errorCode(err: anyerror) c_int {
    // TODO(mitchellh): This is a bad idea because its not stable across
    // code changes. For now we just document that error codes are not
    // stable but that is not useful at all!
    return @intFromError(err);
}

test "c-api sizes" {
    // This tests the sizes that are defined in the C API. We must ensure
    // that our main structure sizes never exceed these so that the C ABI
    // is maintained.
    //
    // THE MAGIC NUMBERS ARE KEPT IN SYNC WITH "include/xev.h"
    const testing = std.testing;
    try testing.expect(@sizeOf(xev.Loop) <= 512);
    try testing.expect(@sizeOf(Completion) <= 320);
    try testing.expect(@sizeOf(xev.Async) <= 256);
    try testing.expect(@sizeOf(xev.Timer) <= 256);
    try testing.expectEqual(@as(usize, 48), @sizeOf(xev.ThreadPool));
    try testing.expectEqual(@as(usize, 24), @sizeOf(xev.ThreadPool.Batch));
    try testing.expectEqual(@as(usize, 24), @sizeOf(Task));
    try testing.expectEqual(@as(usize, 8), @sizeOf(xev.ThreadPool.Config));
}
