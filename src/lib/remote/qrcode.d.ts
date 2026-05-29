declare module 'qrcode' {
  export function toCanvas(
    canvas: HTMLCanvasElement,
    text: string,
    options?: {
      width?: number;
      margin?: number;
      color?: { dark?: string; light?: string };
    },
    cb?: (error: Error | null) => void
  ): Promise<void>;
}
