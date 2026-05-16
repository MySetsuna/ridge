import type { Handle } from '@sveltejs/kit';

const themes: Record<string, { accent: string; bg: string }> = {
  sand: { accent: '#c69a4f', bg: '#faf6ef' },
  grass: { accent: '#6c9a3d', bg: '#f3f8ee' },
  soil: { accent: '#d97757', bg: '#1c1410' },
  wheat: { accent: '#c8860c', bg: '#fdf8e8' },
  starsky: { accent: '#4899ff', bg: '#040810' },
  dark: { accent: '#36c26e', bg: '#071009' }
};

export const handle: Handle = async ({ event, resolve }) => {
  const theme = event.cookies.get('ridge-theme') || 'dark';
  const config = themes[theme] || themes['dark'];

  const response = await resolve(event, {
    transformPageChunk: ({ html }) => {
      // 1. 注入主题属性到 html 标签，确保 app.css 变量立即生效
      let result = html.replace('<html lang="en">', `<html lang="en" data-rg-theme="${theme}">`);
      
      // 2. 注入开机动画专用的变量，并使用独立变量名避免污染
      const styleTag = `
        <style id="ssr-theme">
          :root {
            --startup-accent: ${config.accent};
            --rg-loader-bg: ${config.bg}; 
          }
        </style>
      `;
      return result.replace('%sveltekit.head%', `${styleTag}\n%sveltekit.head%`);
    }
  });

  return response;
};
