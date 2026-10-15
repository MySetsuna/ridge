// Archived frontend call: Runtime already owns and projects the original PTYs.
if (!webRemote) {
  try {
    await withTimeout(
      invoke<number>('reattach_kernel_ptys'),
      15_000,
      'kernel PTY reattach timed out after 15s',
    );
  } catch (e) {
    console.warn('kernel PTY reattach unavailable', e);
  } finally {
    releaseDesktopKernelReattachGate();
  }
} else {
  releaseDesktopKernelReattachGate();
}
