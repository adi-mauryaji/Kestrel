# Speaker notes · Kestrel v3, for a general audience

What to say on each slide of `kestrel_v3_slides.pdf` (or the HTML file: open it in a browser, press F11 for full
screen, then →, Space or a click to go on). 14 slides, about 13 minutes. Every measured number here and on the
slides is filled in from `paper/v3/generated/numbers.json`, which is made from the project's committed test logs;
every quoted request and answer was checked word for word against those logs.

Build: `.venv/bin/python paper/v3/slides/build.py` (this file is made from `speaker_notes.template.md`).

## 1 · Kestrel (about 40 s)

This is Kestrel. It is a personal assistant, like the ones on your phone, with one big difference: it lives
entirely on this laptop. Nothing you type is sent over the internet. Today I'll show you version 3: what it can do, how
well it does it, and what it still gets wrong.

The bird is a kestrel. Kestrels hover in one place and watch closely. That is the idea: an assistant that stays
with you and pays attention, without flying off with your data.

## 2 · Where your words go (about 1 min)

When you ask most assistants a question, your words travel over the internet to a company's computer, get
answered there, and come back. Your question, and often your notes and documents, leave your device.

Kestrel does the whole job here, on the laptop. It works with the wifi switched off. It costs nothing to use: no
subscription and no paid service behind it. It uses open language models, small enough to run on an ordinary
laptop.

## 3 · What it does (about 1 min)

Here is what you can ask it, in English or in Hinglish, the way people actually type. Make tasks and reminders.
Find a note even if you don't remember its words. Ask a question and get the answer from your own notes. Draft an
email. Rewrite or translate a line. Chat or explain something. And it can write and check small Python programs.

The examples in pale yellow are real requests from our tests. The last one matters too: when you ask for
something it cannot do, like today's weather, it says so instead of making something up.

## 4 · Real requests (about 1 min)

These are word-for-word exchanges from the final test before we froze version 3. It ran on a copy of Kestrel with
sample notes, so no real person's notes were used.

A reminder in Hinglish: "kal shaam 6 baje bill bharna yaad dila dena". It made the task for 6 pm the next day,
in 4.8 seconds. A question about the notes: "maa ka birthday kab hai?" It answered in Hinglish and
named the note it used. A translation into Hindi, in Hindi script. And a question the notes cannot answer: it
says so, instead of guessing.

## 5 · How it decides (about 1.5 min)

The hardest part is understanding what you want. Is it a reminder, an email, a question about your notes?

Think of a good receptionist. First, a quick look: a small, fast model reads the request. That takes a few
milliseconds; the same kind of model measured 5.6 milliseconds in our first version. If it is
sure, the request goes straight to the right helper. If it is unsure, it gets a second opinion from a bigger
model, Gemma, which takes about half a second (523 ms on our practice set). If both are unsure,
Kestrel asks you, for example "Write this as an email or message draft?"; only when its first look leans to its
everyday tools does it just use them.

On the test set it got 256/280 decisions right. It asked first 13 times. Only
6 times did it start something nobody asked for, like treating "london mein abhi kitne baje
hain?" as a question about your notes.

Be honest if asked: these 280 test requests were written by us for this project, and most of them had
been used once before, in an earlier check. Only 24 were brand new.

## 6 · Notes by meaning (about 1 min)

Normal search needs the same words. Kestrel can find a note by what it means. Here someone searched for "maa ke
liye tohfa", a gift for mom. The note never says "tohfa"; it says "gift ideas". Still, it found the note about Maa's birthday
with the gift ideas.

At first this worked badly for Hinglish: on our practice searches it found 1/9. The fix was simple:
first turn the Hinglish search into English, then look. After that, with its cut-off tuned again on the same
searches, it found 9/9. On the separate test set it found the right note 14/18 times
(7/9 of the Hinglish ones), and when no note fitted, it added nothing: 0/2 wrong notes.

## 7 · Honest answers (about 1 min)

When you ask a question about your life, Kestrel answers only from your notes. "On which date do I fly to Goa?"
It found the trip note and answered. "What is the name of our hotel in Goa?" The note does not say, so Kestrel
says the notes don't have it. It does not invent a hotel.

On the test set it handled 25/25 questions right: 19/19 answered from the notes, and
6/6 times it rightly said the notes don't have the answer. Before showing an answer, it also checks
that the numbers and names in it really appear in the note. This test is small, 25 questions, and it
tests only the answering step: spotting that a question is about your notes is a separate step, which got
12/16 on its own test. So read it as "very promising", not as "perfect".

## 8 · Code (about 1 min)

Kestrel can also write small Python programs, fix a broken function, or explain code. Once it has written the code
(you see it appear as it is written), it tries it inside a sealed box: no internet, no access to your files, and limits on time and memory. It tells you
what the check found, and if the code breaks there, it tries again, at most twice.

On the test set: functions 6/6, fixes 6/6, explanations 6/6,
but scripts that read what you type only 3/6. In all, 21/24 requests passed our checks;
explanations were checked only for their key points. The box catches mistakes in loading the code; it does not run
your program with your own input.

## 9 · An ordinary laptop (about 1 min)

All of this runs on an ordinary laptop: a Ryzen 5 5500U processor with 6 cores, 14.95 GiB of
memory, and no separate graphics card.

Two changes made version 3 lighter. First, it maps the language model's file instead of copying it into memory,
so the laptop can take most of that memory back when it needs it. The memory it holds and cannot give back fell from 3247 to
1144 MiB, 64.8% less. Second, it saves its warm-up once, so the model is ready in
2.0 seconds instead of 19.1.

## 10 · How long you wait (about 1 min)

These are the times from the test run, from pressing enter to the complete answer. Things Kestrel does with its
own tools, like a reminder or a search, mostly take under a second (0.3 seconds for this reminder);
some Hinglish requests take a few seconds (4.8 for a Hinglish reminder). When the language model writes, it takes
longer, because it writes word by word: 2.8 seconds for a short translation, 8.5 for
an email, 23.2 for a long explanation. Chat, emails, translations and code appear as they are
written, so you start reading long before the end; answers from your notes and note summaries appear once
they are checked.

## 11 · Tested (about 1 min)

Before we froze version 3, we tested every feature the way a user would. 34/36 everyday requests passed
from start to finish. The web page passed 8/8 checks in a hidden browser and
8/8 on a real screen; that is a screenshot from the test. A reminder set for 17:31
popped up at 17:31:02. And 448 automatic checks of Kestrel's code passed, plus 510 of the scripts that test it.

I'll come to the two requests that failed on the next slide.

## 12 · Not yet (about 1 min)

Here is what it cannot do well yet. Some questions about your notes are refused: "how much is my rent?" got "I
can't do that", although the rent is in the notes. That was one of the two failures. The other: asked "which
preferences have I set?", it found nothing, although one was saved. Programs that read what you type often fail.
It does not remember the conversation, so "continue" does not work. Some Hinglish questions get English answers.
Bold text shows with its stars. And all our tests were small and written by us. Real people's requests are the
next real test.

## 13 · Next (about 45 s)

Next, we learn from real mistakes: collect the requests it gets wrong in daily use, teach it with them, and test
again on questions it has never seen. We close the gaps from the last slide, each with its own test. And version
4 moves to a bigger machine, with larger models, still offline and still private.

## 14 · Thank you (about 15 s)

Your words stay on your laptop. Thank you. I'm happy to take questions.

## Likely questions

**"Is it as good as ChatGPT?"** No, and it does not try to be. It uses much smaller models so it can run on a
laptop without the internet. In our tests it handled everyday personal tasks well (34/36 of our end-to-end
requests), but the tests were small; for long, expert answers a large online model is better. The trade is privacy and zero cost.

**"How do you know the numbers are right?"** Every measured number on these slides is filled in automatically from
the saved test logs, and every quoted answer was checked word for word. But the tests are small and we wrote them ourselves, so treat the numbers as
a careful first check, not a final verdict.

**"Can it make mistakes?"** Yes. Chat answers carry a note that a small offline model wrote them and may be wrong.
Notes answers are checked against the note before they are shown, and code it writes as code is tried in the
sealed box, but none of this makes it perfect. Slide 12 lists the main things we know go wrong.
