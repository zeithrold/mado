import { app } from '@tauri-apps/api'
import { arch, type } from '@tauri-apps/plugin-os'
import useSWRV from 'swrv'

/**
 * Fetches operating system information such as type and architecture.
 * This function uses the `@tauri-apps/plugin-os` plugin to retrieve the OS
 * type (e.g., 'Windows', 'Linux', 'macOS') and architecture (e.g., 'x64', 'arm64').
 * It returns an object containing the OS type and architecture.
 */
export function useOSInfo() {
  return useSWRV('mado/meta/os-info', async () => {
    const [
      osType,
      osArch,
    ] = await Promise.all([
      type(),
      arch(),
    ])
    return {
      type: osType,
      arch: osArch,
    }
  })
}

/**
 * Fetches application metadata such as version, Tauri version, name, and bundle type.
 */
export function useAppInfo() {
  return useSWRV('mado/meta/app-info', async () => {
    const [
      version,
      tauriVersion,
      name,
      bundleType,
    ]
    = await Promise.all([
      app.getVersion(),
      app.getTauriVersion(),
      app.getName(),
      app.getBundleType(),
    ])
    return {
      version,
      tauriVersion,
      name,
      bundleType,
    }
  })
}
