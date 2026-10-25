#!/bin/sh
# Makes the JPEG and TIFF samples of tools/fixtures/modules/render.image.js (ImageMagick 7 and libjpeg-turbo's
# cjpeg). Run once; the samples are committed, so the fixture never depends on these tools.
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# A picture with edges and noise, sizes that are not whole blocks or MCUs.
magick -seed 7 -size 21x13 plasma:fractal -depth 8 "$tmp/src.png"
magick "$tmp/src.png" -alpha set -channel A -fx 'i/w' +channel "$tmp/alpha.png"
magick "$tmp/src.png" "$tmp/src.ppm"
magick "$tmp/src.png" -colorspace Gray "$tmp/src.pgm"

j() { out=$1; shift; cjpeg "$@" -outfile "$out.jpg" "$tmp/src.ppm"; }
j base444 -quality 90 -sample 1x1
j base420 -quality 75 -sample 2x2
j base422 -quality 75 -sample 2x1
j base411 -quality 60 -sample 4x1
j base440 -quality 60 -sample 1x2
j optimized -optimize -sample 2x2
j restart1 -restart 1B -sample 2x2
j restart3 -restart 3B -sample 1x1
j rgb -rgb
j prog420 -progressive -sample 2x2
j prog444 -progressive -sample 1x1
j progrestart -progressive -restart 2B -sample 2x2
j arith -arithmetic
j lossless -lossless 1
j q100 -quality 100 -sample 1x1
j q5 -quality 5 -sample 2x2
cjpeg -grayscale -outfile grey.jpg "$tmp/src.pgm"
cjpeg -grayscale -progressive -outfile greyprog.jpg "$tmp/src.pgm"
magick "$tmp/src.png" -colorspace CMYK -quality 80 cmyk.jpg
magick "$tmp/src.png" -colorspace CMYK -interlace JPEG -quality 80 cmykprog.jpg

t() { out=$1; src=$2; shift 2; magick "$tmp/$src" "$@" "$out.tif"; }
t none src.png -compress None
t lzw src.png -compress LZW -define tiff:predictor=1
t lzwpred src.png -compress LZW -define tiff:predictor=2
t zip src.png -compress Zip
t zippred src.png -compress Zip -define tiff:predictor=2
t packbits src.png -compress RLE
t strips src.png -compress LZW -define tiff:rows-per-strip=2
t msb src.png -compress None -endian MSB
t msbpred16 src.png -compress Zip -define tiff:predictor=2 -depth 16 -endian MSB
t lsb16 src.png -compress None -depth 16 -endian LSB
t lsbpred16 src.png -compress LZW -define tiff:predictor=2 -depth 16 -endian LSB
t grey src.pgm -compress None
t greypred src.pgm -compress LZW -define tiff:predictor=2
t grey4 src.pgm -compress None -depth 4
t grey2 src.pgm -compress RLE -depth 2
t mono src.pgm -compress None -threshold 50% -depth 1 -type Bilevel
t miniswhite src.pgm -compress None -threshold 50% -depth 1 -type Bilevel -define quantum:polarity=min-is-white
t palette src.png -compress None -type Palette -colors 13
t palette4 src.png -compress None -type Palette -colors 9 -depth 4
t alpha alpha.png -compress LZW
t alpha16 alpha.png -compress None -depth 16
t assoc alpha.png -compress None -define tiff:alpha=associated
t grey16 src.pgm -compress LZW -depth 16
t greyalpha alpha.png -colorspace Gray -compress None
t cmyk src.png -colorspace CMYK -compress None
t jpegtiff src.png -compress JPEG
t planar src.png -compress None -interlace Plane
