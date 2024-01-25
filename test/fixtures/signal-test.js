let i = 3

process.on('SIGINT', function () {
  if (i > 0) {
    console.log(`Got SIGINT.  Press Control-D to exit. ${i} times left`)
    i--
  } else {
    process.exit()
  }
})

// wait for 60 seconds
setTimeout(function () {}, 60000)
console.log('Running.  Press Control-C to test.')
