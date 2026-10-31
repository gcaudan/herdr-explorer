@echo off
rem A fake herdr for the tests on Windows: the same as the sh script next to it (calls logged to
rem %FAKE_HERDR_DIR%\calls.log, answers from %FAKE_HERDR_DIR%\<first>-<second>). Arguments are read one by one
rem (shift), so that commas, equal signs and wildcards stay as they are.
if not defined FAKE_HERDR_DIR exit /b 1
set "answer=%FAKE_HERDR_DIR%\%~1-%~2"
set "line=%~1"
shift
:next
if "%~1"=="" goto done
set "line=%line%	%~1"
shift
goto next
:done
>>"%FAKE_HERDR_DIR%\calls.log" echo(%line%
if exist "%answer%" (type "%answer%") else (echo {"result":null})
