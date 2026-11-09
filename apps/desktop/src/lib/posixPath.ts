/** A relative path with forward slashes on every OS: `readdirSync` yields backslashes on Windows. */
export function toPosixPath(name: string): string {
  return name.replaceAll("\\", "/");
}
