// Skip Husky install in production and CI
if (process.env.NODE_ENV === 'production' || process.env.CI === 'true') {
    process.exit(0)
}
const husky = (await import('husky')).default

try {
    console.log(husky())
} catch (error: any) {
    // Ignore EEXIST errors on Windows when directory already exists
    if (error?.code !== 'EEXIST') {
        throw error
    }
}

