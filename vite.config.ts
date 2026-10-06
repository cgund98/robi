import react from '@vitejs/plugin-react'
import { defineConfig, type Plugin } from 'vitest/config'
import solid from 'vite-plugin-solid'
// @ts-expect-error type error without @types/node package
import process from 'node:process'

const host = process.env.TAURI_DEV_HOST

// Tauri sets this for `beforeDevCommand` and `beforeBuildCommand`. Desktop builds
// compile the Solid tree only until the React shell is the one we ship again.
const solidOnly = Boolean(process.env.TAURI_ENV_PLATFORM)

function solidIndex(): Plugin {
  return {
    name: 'robi-solid-index',
    // vite-plugin-solid prefers the `solid` export, which is uncompiled JSX that
    // re-exports with `export *`. WebKit cannot import `default` through that
    // and the window stays blank. Drop the condition so Tauri loads the
    // prebuilt ESM instead. Our own `.tsx` is still compiled by the plugin.
    // vite-plugin-solid adds the `solid` condition in configEnvironment, which
    // loads uncompiled JSX. Strip it on the environment Vite actually uses.
    configEnvironment(_name, config) {
      if (!config.resolve?.conditions) {
        return
      }
      config.resolve.conditions = config.resolve.conditions.filter(
        (condition) => condition !== 'solid'
      )
    },
    // vite-plugin-solid excludes every package that has a `solid` export so it
    // can compile that JSX. That leaves `export *` files for WebKit. The React
    // app prebundles its dependencies instead. Drop the exclusion so these
    // resolve to their prebuilt JS and land in `.vite/deps` too.
    configResolved(config) {
      config.optimizeDeps.exclude = (config.optimizeDeps.exclude ?? []).filter((dep) =>
        dep.startsWith('@tauri-apps/')
      )
    },
    transformIndexHtml: {
      order: 'pre',
      handler(html) {
        return html
          .replace('    <div id="solid-root"></div>\n', '')
          .replace(/src="\/src\/main\.tsx[^"]*"/, 'src="/src/solid/main.tsx"')
      }
    },
    // WebKit throws "Importing binding name 'default' cannot be resolved by star
    // export entries" when a default import crosses `export *`. Rewrite that
    // before the webview loads the module. Prebundled deps skip this hook;
    // `webkitRolldownPlugin` covers those chunks.
    transform: {
      order: 'pre',
      handler(code, id) {
        const file = id.split('?')[0]
        if (file.includes('.module.css')) {
          if (code.includes('export default')) {
            return
          }
          const names = [...code.matchAll(/export const (\w+)/g)].map((match) => match[1])
          if (names.length === 0) {
            return
          }
          return `${code}\nexport default { ${names.join(', ')} }\n`
        }
        if (file.includes('/src/solid/') && (file.endsWith('.tsx') || file.endsWith('.ts'))) {
          const rewritten = code.replace(
            /import\s+(\w+)\s+from\s+(['"][^'"]+\.module\.css(?:\?[^'"]*)?['"])/g,
            'import * as $1 from $2'
          )
          return rewritten === code ? undefined : rewritten
        }
        if (!file.includes('node_modules') || file.includes('/.vite/')) {
          return
        }
        return rewriteWebKitExports(code)
      }
    }
  }
}

// A local `export default` resolves in WebKit. `export { default } from`
// beside `export * from` does not: WebKit looks `default` up in the star list.
function rewriteWebKitExports(code: string): string | undefined {
  if (!code.includes('*') && !code.includes('default')) {
    return
  }
  let n = 0
  let next = code.replace(
    /export\s*\{\s*default\s*\}\s*from\s*(['"][^'"]+['"]);?/g,
    (_match, spec: string) => {
      const local = `__webkitDefault${n++}`
      return `import ${local} from ${spec}\nexport default ${local}`
    }
  )
  if (!next.includes('export default')) {
    let addedDefault = false
    next = next.replace(/export\s*\*\s*from\s*(['"][^'"]+['"]);?/g, (match, spec: string) => {
      if (addedDefault) {
        return match
      }
      addedDefault = true
      const local = `__webkitStar${n++}`
      return `import * as ${local} from ${spec}\nexport default ${local}.default\n${match}`
    })
  }
  return next === code ? undefined : next
}

function webkitRolldownPlugin() {
  return {
    name: 'robi-webkit-exports',
    renderChunk(code: string) {
      const rewritten = rewriteWebKitExports(code)
      return rewritten ? { code: rewritten, map: null } : null
    }
  }
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: solidOnly
    ? [solid({ include: /\/src\/solid\/.*\.tsx?$/ }), solidIndex()]
    : [solid({ include: /\/src\/solid\/.*\.tsx?$/ }), react({ exclude: /\/src\/solid\// })],
  // Same dep prebundle as the React app: scan from the Solid entry and bundle
  // packages into `.vite/deps`, so WebKit never links raw `export *` files.
  optimizeDeps: solidOnly
    ? {
        entries: ['src/solid/main.tsx'],
        include: ['solid-markdown', 'remark-gfm'],
        exclude: ['@tauri-apps/api', '@tauri-apps/plugin-dialog'],
        rolldownOptions: { plugins: [webkitRolldownPlugin()] }
      }
    : undefined,
  // Tauri's macOS webview is WebKit. Same target as
  // https://github.com/riipandi/tauri-start-solid — downlevel the bundle so
  // `export *` plus a default import is not left for WebKit to reject.
  build: solidOnly
    ? {
        target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
        minify: process.env.TAURI_ENV_DEBUG ? false : 'oxc',
        sourcemap: !!process.env.TAURI_ENV_DEBUG
      }
    : undefined,

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1431
        }
      : undefined,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:1431',
        changeOrigin: true
      }
    },
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**']
    }
  },
  test: {
    environment: 'happy-dom',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx']
  }
}))
