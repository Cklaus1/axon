.PHONY: check test demo
check:
	python tools/check_pack.py
test:
	python -m unittest discover -s tests -v
demo:
	python -m harness.simulate --case document
